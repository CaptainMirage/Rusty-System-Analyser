use super::{
    constants::*,
    utils::*,
    types::* 
};
use chrono::{DateTime, Duration, NaiveDateTime, TimeZone, Utc};
use rayon::prelude::*;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    io::{self, Error},
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::Path,
};
use walkdir::WalkDir;
use winapi::um::{
    fileapi::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDriveStringsW},
    winbase::DRIVE_FIXED,
};

pub struct StorageAnalyzer {
    pub drives: Vec<String>,
    file_cache: HashMap<String, Vec<FileInfo>>,
}

impl StorageAnalyzer {
    pub fn new() -> Self {
        let drives = Self::list_drives();
        StorageAnalyzer {
            drives,
            file_cache: HashMap::new(),
        }
    }

    // Windows-specific implementation to list fixed drives
    // Filters for physical drives only, skips USB/network drives
    #[cfg(target_os = "windows")]
    fn list_drives() -> Vec<String> {
        let mut buffer = [0u16; 256];
        let len = unsafe { GetLogicalDriveStringsW(buffer.len() as u32, buffer.as_mut_ptr()) };

        if len == 0 {
            return Vec::new();
        }

        buffer[..len as usize]
            .split(|&c| c == 0)
            .filter_map(|slice| {
                (!slice.is_empty())
                    .then(|| {
                        let drive = OsString::from_wide(slice);
                        let drive_type = unsafe { GetDriveTypeW(slice.as_ptr()) };
                        (drive_type == DRIVE_FIXED).then(|| drive.to_string_lossy().into_owned())
                    })
                    .flatten()
            })
            .collect()
    }

    // Placeholder for non-Windows platforms
    #[cfg(not(target_os = "windows"))]
    fn list_drives() -> Vec<String> {
        Vec::new()
    }

    // Uses Windows API to get drive space information
    fn get_drive_space(&self, drive: &str) -> io::Result<DriveAnalysis> {
        use winapi::um::winnt::ULARGE_INTEGER;
        let mut free_bytes_available: ULARGE_INTEGER = unsafe { std::mem::zeroed() };
        let mut total_bytes: ULARGE_INTEGER = unsafe { std::mem::zeroed() };
        let mut total_free_bytes: ULARGE_INTEGER = unsafe { std::mem::zeroed() };

        // Convert drive path to wide string for Windows API
        let wide_drive: Vec<u16> = OsStr::new(drive).encode_wide().chain(Some(0)).collect();

        let success = unsafe {
            GetDiskFreeSpaceExW(
                wide_drive.as_ptr(),
                &mut free_bytes_available as *mut _ as *mut _,
                &mut total_bytes as *mut _ as *mut _,
                &mut total_free_bytes as *mut _ as *mut _,
            )
        };

        if success == 0 {
            return Err(Error::last_os_error());
        }

        let total_size = unsafe { *total_bytes.QuadPart() } as f64 / GB_TO_BYTES;
        let free_space = unsafe { *total_free_bytes.QuadPart() } as f64 / GB_TO_BYTES;
        let used_space = total_size - free_space;

        Ok(DriveAnalysis {
            total_size,
            used_space,
            free_space,
            free_space_percent: (free_space / total_size) * 100.0,
        })
    }

    // Uses parallel processing for better performance on large directories
    fn collect_and_cache_files(&mut self, drive: &str) -> io::Result<()> {
        if self.file_cache.contains_key(drive) {
            return Ok(());
        }

        let files: Vec<FileInfo> = WalkDir::new(drive)
            .into_iter()
            .par_bridge() // Enable parallel processing
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
            .filter_map(|entry| {
                entry
                    .metadata()
                    .ok()
                    .map(|metadata| {
                        let file_size = metadata.len() as f64 / MB_TO_BYTES;
                        let last_modified =
                            metadata.modified().ok().map(system_time_to_string);
                        let last_accessed =
                            metadata.accessed().ok().map(system_time_to_string);

                        last_modified.map(|modified| FileInfo {
                            full_path: entry.path().to_string_lossy().to_string(),
                            size_mb: file_size,
                            last_modified: modified,
                            last_accessed,
                        })
                    })
                    .flatten()
            })
            .collect();

        self.file_cache.insert(drive.to_string(), files);
        Ok(())
    }

    fn get_file_type_distribution(&mut self, drive: &str) -> io::Result<Vec<(String, f64, usize)>> {
        self.collect_and_cache_files(drive)?;

        let file_types: HashMap<String, FileTypeStats> =
            if let Some(files) = self.file_cache.get(drive) {
                files
                    .par_iter()
                    .fold(
                        || HashMap::new(),
                        |mut acc, file_info| {
                            let ext = Path::new(&file_info.full_path)
                                .extension()
                                .map(|e| e.to_string_lossy().to_lowercase())
                                .unwrap_or_else(|| "(No Extension)".to_string());

                            let size = (file_info.size_mb * MB_TO_BYTES) as u64;

                            let stats: &mut FileTypeStats = acc.entry(ext).or_default();
                            stats.total_size += size;
                            stats.count += 1;
                            acc
                        },
                    )
                    .reduce(
                        || HashMap::new(),
                        |mut acc1, acc2| {
                            for (ext, stats2) in acc2 {
                                let stats1 = acc1.entry(ext).or_default();
                                stats1.total_size += stats2.total_size;
                                stats1.count += stats2.count;
                            }
                            acc1
                        },
                    )
            } else {
                HashMap::new()
            };

        let mut distribution: Vec<_> = file_types
            .into_iter()
            .map(|(ext, stats)| (ext, stats.total_size as f64 / GB_TO_BYTES, stats.count))
            .filter(|&(_, size, _)| size > MIN_FILE_TYPE_SIZE_GB)
            .collect();

        distribution.par_sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        Ok(distribution)
    }

    fn get_largest_files(&mut self, drive: &str) -> io::Result<Vec<FileInfo>> {
        self.collect_and_cache_files(drive)?;

        if let Some(files) = self.file_cache.get(drive) {
            let mut result = files.clone();
            result.par_sort_unstable_by(|a, b| b.size_mb.partial_cmp(&a.size_mb).unwrap());
            Ok(result)
        } else {
            Ok(Vec::new())
        }
    }
    
    // Main analysis function that calls all the other functions below
    pub fn analyze_drive(&mut self, drive: &str) -> io::Result<()> {
        println!("\n=== Storage Distribution Analysis ===");
        println!("Date: {}", Utc::now().format(DATE_FORMAT));
        println!("Drive: {}", drive);

        self.print_drive_analysis(drive)?;
        self.print_largest_folders(drive)?;
        self.print_file_type_distribution(drive)?;
        self.print_largest_files(drive)?;
        self.print_recent_large_files(drive)?;
        self.print_old_large_files(drive)?;

        Ok(())
    }

    fn print_drive_analysis(&self, drive: &str) -> io::Result<()> {
        match self.get_drive_space(drive) {
            Ok(analysis) => {
                println!("\n--- Drive Space Overview ---");
                println!("Total Size: {:.2} GB", analysis.total_size);
                println!("Used Space: {:.2} GB", analysis.used_space);
                println!("Free Space: {:.2} GB ({:.2}%)", analysis.free_space, analysis.free_space_percent);
                Ok(())
            }
            Err(e) => {
                eprintln!("Failed to analyze drive '{}': {}", drive, e);
                Err(e)
            }
        }
    }

    // Analyzes and returns largest folders up to 3 levels deep
    // Excludes hidden folders (those starting with '.')
    fn print_largest_folders(&self, drive: &str) -> io::Result<()> {
        println!("\n--- Largest Folders (Top 10) ---");
        let largest_folders = self.get_largest_folders(drive)?;
        let mut cnt: i8 = 0;
        for folder in largest_folders.iter().take(10) {
            cnt += 1;
            println!("\n[{}] {}", cnt, folder.folder);
            println!("  Size: {:.2} GB", folder.size_gb);
            println!("  Files: {}", folder.file_count);
        }
        Ok(())
    }

    fn print_file_type_distribution(&mut self, drive: &str) -> io::Result<()> {
        println!("\n--- File Type Distribution (Top 10) ---");
        let distribution = self.get_file_type_distribution(drive)?;
        for (ext, size, count) in distribution.iter().take(10) {
            println!(
                "\n[>] {} \n  Count: {} \n  Size: {:.2} GB",
                ext, count, size
            );
        }
        Ok(())
    }

    fn get_largest_folders(&self, drive: &str) -> io::Result<Vec<FolderSize>> {
        let mut folders = WalkDir::new(drive)
            .min_depth(1)
            .max_depth(3)
            .into_iter()
            .par_bridge()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_dir())
            .filter(|e| {
                !e.file_name()
                    .to_str()
                    .map(|s| s.starts_with('.'))
                    .unwrap_or(false)
            })
            .filter_map(|entry| {
                self.calculate_folder_size(entry.path())
                    .ok()
                    .filter(|size| size.size_gb > MIN_FOLDER_SIZE_GB)
            })
            .collect::<Vec<_>>();

        folders.par_sort_unstable_by(|a, b| b.size_gb.partial_cmp(&a.size_gb).unwrap());
        Ok(folders)
    }

    fn calculate_folder_size(&self, path: &Path) -> io::Result<FolderSize> {
        let files: Vec<_> = WalkDir::new(path)
            .into_iter()
            .par_bridge()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
            .collect();

        let total_size: u64 = files
            .par_iter()
            .map(|entry| entry.metadata().map(|m| m.len()).unwrap_or(0))
            .sum();

        Ok(FolderSize {
            folder: path.to_string_lossy().to_string(),
            size_gb: total_size as f64 / GB_TO_BYTES,
            file_count: files.len(),
        })
    }

    fn print_largest_files(&mut self, drive: &str) -> io::Result<()> {
        println!("\n--- Largest Files ---");
        let files = self.get_largest_files(drive)?;
        for file in files.iter().take(10) {
            println!("\n[*] Path: {}", file.full_path);
            println!("    Size: {:.2} MB / {:.2} GB", file.size_mb, file.size_mb/1000.0);
            println!("    Last Modified: {}", file.last_modified);
            if let Some(last_accessed) = &file.last_accessed {
                println!("    Last Accessed: {}", last_accessed);
            }
        }
        Ok(())
    }

    // NOTE - fixed this shit
    // Gets recently modified large files (within last 30 days)
    fn get_recent_large_files(&mut self, drive: &str) -> io::Result<Vec<FileInfo>> {
        self.collect_and_cache_files(drive)?;

        let mut files = if let Some(files) = self.file_cache.get(drive) {
            files.clone()
        } else {
            return Ok(Vec::new());
        };

        let thirty_days_ago = Utc::now().naive_utc() - Duration::days(30);

        files.retain(|file| {
            NaiveDateTime::parse_from_str(&file.last_modified, DATE_FORMAT)
                .map(|dt| dt > thirty_days_ago)
                .unwrap_or(false)
        });

        files.sort_by(|a, b| b.size_mb.partial_cmp(&a.size_mb).unwrap());
        Ok(files)
    }

    fn print_recent_large_files(&mut self, drive: &str) -> io::Result<()> {
        println!("\n--- Recent Large Files ---");
        let files = self.get_recent_large_files(drive)?;
        for file in files.iter().take(10) {
            println!("\n[*] Path: {}", file.full_path);
            println!("    Size: {:.2} MB / {:.2} GB", file.size_mb, file.size_mb/1000.0);
            println!("    Last Modified: {}", file.last_modified);
            if let Some(last_accessed) = &file.last_accessed {
                println!("    Last Accessed: {}", last_accessed);
            }
        }
        Ok(())
    }

    // NOTE - fixed this shit also
    // Gets old large files (older than 6 months)
    fn get_old_large_files(&mut self, drive: &str) -> io::Result<Vec<FileInfo>> {
        self.collect_and_cache_files(drive)?;

        let mut files = if let Some(files) = self.file_cache.get(drive) {
            files.clone()
        } else {
            return Ok(Vec::new());
        };

        let six_months_ago = Utc::now().naive_utc() - Duration::days(180);

        files.retain(|file| {
            NaiveDateTime::parse_from_str(&file.last_modified, DATE_FORMAT)
                .map(|dt| dt < six_months_ago)
                .unwrap_or(false)
        });

        files.sort_by(|a, b| b.size_mb.partial_cmp(&a.size_mb).unwrap());
        Ok(files)
    }

    fn print_old_large_files(&mut self, drive: &str) -> io::Result<()> {
        println!("\n--- Old Large Files (>6 months old) ---");
        let files = self.get_old_large_files(drive)?;
        for file in files.iter().take(10) {
            println!("\n[*] Path: {}", file.full_path);
            println!("    Size: {:.2} MB / {:.2}", file.size_mb, file.size_mb/1000.0);
            println!("    Last Modified: {}", file.last_modified);
            if let Some(last_accessed) = &file.last_accessed {
                println!("    Last Accessed: {}", last_accessed);
            }
        }
        Ok(())
    }
}

