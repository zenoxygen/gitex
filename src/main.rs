use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::path::PathBuf;
use std::str;

use anyhow::{anyhow, Result};
use git2::{Commit, Oid, Repository, Tree};
use indicatif::{ProgressBar, ProgressStyle};
use log::info;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
#[structopt(about = "Extract data from a Git repository.")]
struct Config {
    #[structopt(
        long,
        parse(from_os_str),
        required = true,
        help = "Path to the Git repository"
    )]
    repository: PathBuf,
    #[structopt(long, parse(from_os_str), help = "Path to the output file")]
    output: PathBuf,
    #[structopt(
        long,
        use_delimiter = true,
        value_delimiter = ",",
        required = true,
        help = "List of file extensions (comma-separated)"
    )]
    extensions: Vec<String>,
    #[structopt(long, help = "Size of the dataset")]
    size: usize,
    #[structopt(long, default_value = "8", help = "Minimum commit message length")]
    message_len_min: usize,
    #[structopt(long, default_value = "64", help = "Maximum commit message length")]
    message_len_max: usize,
    #[structopt(long, default_value = "1", help = "Minimum commit changes length")]
    changes_len_min: usize,
    #[structopt(long, default_value = "1024", help = "Maximum commit changes length")]
    changes_len_max: usize,
    #[structopt(long, help = "Show progress bar")]
    show_progress: bool,
}

struct Record {
    /// Contains a commit message.
    commit_message: String,
    /// Contains commit changes.
    commit_changes: String,
}

struct Extractor {
    /// Configuration.
    config: Config,
    /// The Git repository to analyze.
    git_repo: Repository,
    /// Output file to save the dataset.
    output_file: File,
    /// Set target of file extensions.
    file_extensions: HashSet<OsString>,
    /// Extracted data from commits.
    records: Vec<Record>,
    /// Set of commit ids processed.
    processed_commit_ids: HashSet<Oid>,
    /// Number of commits saved in the dataset.
    nb_commits_saved: usize,
    /// Progress bar.
    progress_bar: ProgressBar,
}

impl Extractor {
    /// Create a new `Extractor` instance with the given configuration.
    fn new(config: Config) -> Result<Extractor> {
        // Open the Git repository
        let git_repo = Repository::open(&config.repository)
            .map_err(|e| anyhow!("failed to open the Git repository ({e})"))?;

        // Open the output file
        let output_file = OpenOptions::new()
            .write(true)
            .create(true)
            .append(true)
            .open(&config.output)
            .map_err(|e| anyhow!("failed to open the output file ({e})"))?;

        // Convert file extensions to a HashSet
        let file_extensions = config.extensions.iter().map(OsString::from).collect();

        // Hold extracted data from commits
        let records = Vec::new();

        // Hold processed commit ids
        let processed_commit_ids = HashSet::new();

        // Hold number of commits saved
        let nb_commits_saved = 0;

        // Create progress bar
        let progress_bar = if config.show_progress {
            ProgressBar::new(config.size as u64)
        } else {
            ProgressBar::hidden()
        };

        // Configure progress bar
        progress_bar.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len}",
            )
            .unwrap()
            .progress_chars("#>-"),
        );

        let extractor = Extractor {
            config,
            git_repo,
            output_file,
            file_extensions,
            records,
            processed_commit_ids,
            nb_commits_saved,
            progress_bar,
        };

        Ok(extractor)
    }

    /// Get message of a given Git commit.
    /// If the commit message header does not match the required length, return None.
    fn get_commit_message(&self, commit: &Commit) -> Option<String> {
        // Get first line (commit summary)
        let first_line = commit.message()?.lines().next()?;

        // Check first line length
        if first_line.len() < self.config.message_len_min
            || first_line.len() > self.config.message_len_max
        {
            return None;
        }

        Some(first_line.to_string())
    }

    /// Get changes of a given Git commit.
    /// If files with others extensions than the target extensions contain changes, return None.
    fn get_commit_changes(&self, commit_tree: &Tree, parent_tree: &Tree) -> Result<Option<String>> {
        let mut commit_changes = String::with_capacity(self.config.changes_len_max);
        let mut files_with_target_extensions_changed = false;
        let mut files_with_other_extensions_changed = false;

        // Create a diff representing the difference between the parent tree and the commit tree
        let diff_output = self
            .git_repo
            .diff_tree_to_tree(Some(parent_tree), Some(commit_tree), None)
            .map_err(|e| anyhow!("failed to create diff ({e})"))?;

        // Iterate over the diff, analyzing each file changed
        diff_output
            .print(git2::DiffFormat::Patch, |delta, _hunk, line_diff| {
                if let Some(file_path) = delta.new_file().path() {
                    // Check if the file extension matches one of the target file extensions
                    if file_path
                        .extension()
                        .map(|ext| self.file_extensions.contains(ext))
                        .unwrap_or(false)
                    {
                        files_with_target_extensions_changed = true;
                        if let Ok(line_diff_content) = str::from_utf8(line_diff.content()) {
                            // Get commit changes
                            commit_changes.push(line_diff.origin());
                            commit_changes.push_str(line_diff_content);
                        }
                    } else {
                        files_with_other_extensions_changed = true;
                    }
                }
                true
            })
            .map_err(|e| anyhow!("failed to parse diff output ({e})"))?;

        // Check if only files with target extensions were changed
        if !files_with_target_extensions_changed || files_with_other_extensions_changed {
            return Ok(None);
        }

        Ok(Some(commit_changes))
    }

    /// Process a commit.
    fn process_commit(&self, commit: &Commit) -> Result<Option<Record>> {
        let commit_oid = commit.id();

        // Check if commit already processed
        if self.processed_commit_ids.contains(&commit_oid) {
            info!("Skip commit #{commit_oid} (already processed)");
            return Ok(None);
        }

        // Check if commit has parents
        if commit.parent_count() == 0 {
            info!("Skip commit #{commit_oid} (no parents)");
            return Ok(None);
        }

        // Get commit parent
        let parent = match commit.parent(0) {
            Ok(parent) => parent,
            Err(_) => {
                info!("Skip commit #{commit_oid} (failed to fetch parent)");
                return Ok(None);
            }
        };

        let commit_tree = commit.tree()?;
        let parent_tree = parent.tree()?;

        // Check if bot commit
        if commit
            .author()
            .name()
            .map(|name| name.to_lowercase().contains("bot"))
            .unwrap_or(false)
        {
            info!("Skip commit #{commit_oid} (commit author indicates a bot)");
            return Ok(None);
        }

        // Get commit message
        let commit_message = match self.get_commit_message(commit) {
            Some(message) => message,
            None => {
                info!("Skip commit #{commit_oid} (commit message out of required length)");
                return Ok(None);
            }
        };

        // Check if commit message indicates a merge
        if commit_message.starts_with("Merge pull request")
            || commit_message.starts_with("Merge branch")
        {
            info!("Skip commit #{commit_oid} (commit message indicates a merge)");
            return Ok(None);
        }

        // Get commit changes
        let commit_changes = match self.get_commit_changes(&commit_tree, &parent_tree) {
            Ok(Some(changes)) => changes,
            Ok(None) => {
                info!("Skip commit #{commit_oid} (no changes in files with target extensions)");
                return Ok(None);
            }
            Err(_) => {
                info!("Skip commit #{commit_oid} (failed to read commit changes)");
                return Ok(None);
            }
        };

        // Check commit changes length
        if commit_changes.len() < self.config.changes_len_min
            || commit_changes.len() > self.config.changes_len_max
        {
            info!("Skip commit #{commit_oid} (commit changes out of required length)");
            return Ok(None);
        }

        // Create a new record for this commit
        let record = Record {
            commit_message,
            commit_changes,
        };

        Ok(Some(record))
    }

    /// Save the dataset to the output file as CSV.
    fn save_dataset(&mut self) -> Result<()> {
        // Check if the output file is empty
        let write_header = self.output_file.metadata()?.len() == 0;

        // Write header
        let mut wtr = csv::Writer::from_writer(&self.output_file);
        if write_header {
            wtr.write_record(["commit_message", "commit_changes"])
                .map_err(|e| anyhow!("failed to write csv header ({e})"))?;
        }

        // Write records
        for record in &self.records {
            wtr.write_record([&record.commit_message, &record.commit_changes])
                .map_err(|e| anyhow!("failed to write csv record ({e})"))?;
        }
        wtr.flush()?;

        Ok(())
    }

    /// Run the main logic of the extractor by iterating through the Git commits.
    fn run(&mut self) -> Result<()> {
        // Create revwalk to iterate on commits
        let mut revwalk = self.git_repo.revwalk()?;
        revwalk.push_head()?;

        for commit_oid in revwalk {
            // Check if dataset size has been reached
            if self.nb_commits_saved >= self.config.size {
                break;
            }

            let commit_oid = commit_oid?;
            let commit = self.git_repo.find_commit(commit_oid)?;

            // Check if merge commit
            if commit.parent_count() > 1 {
                // Process only commit parents
                for parent_index in 0..commit.parent_count() {
                    if let Ok(parent) = commit.parent(parent_index) {
                        if let Ok(Some(record)) = self.process_commit(&parent) {
                            info!("Save commit #{:?}", parent.id());
                            self.records.push(record);
                            self.nb_commits_saved += 1;
                            self.progress_bar.inc(1);
                        }
                        self.processed_commit_ids.insert(parent.id());
                    }
                }
            } else {
                // Process normal commit
                if let Ok(Some(record)) = self.process_commit(&commit) {
                    info!("Save commit #{:?}", commit.id());
                    self.records.push(record);
                    self.nb_commits_saved += 1;
                    self.progress_bar.inc(1);
                }
                self.processed_commit_ids.insert(commit.id());
            }
        }

        // Save dataset in output file
        self.save_dataset()?;

        // Finish and clear progress bar
        self.progress_bar.finish_and_clear();

        println!(
            "Total commits processed: {}",
            self.processed_commit_ids.len()
        );
        println!("Total commits saved: {}", self.nb_commits_saved);

        Ok(())
    }
}

fn main() -> Result<()> {
    pretty_env_logger::init_timed();

    // Parse arguments
    let config = Config::from_args();

    // Extract data from commits
    let mut extractor = Extractor::new(config)?;
    extractor.run()?;

    Ok(())
}
