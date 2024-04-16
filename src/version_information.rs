use std::env;

pub const VERSION_MAJOR: i32 = 0;
pub const VERSION_MINOR: i32 = 1;
pub const VERSION_BUILD: i32 = 1;

/// this struct hold data structure and implementation for retrieving and storing version information of the application
pub struct VersionInformation {
    pub major: i32,
    pub minor: i32,
    pub build: i32,
    pub hash: String,
}

impl VersionInformation {
    /// The function creates an instance of VersionInformation.
    pub fn new() -> VersionInformation {
        // get fully qualified path and file name
        let pathfilename = match env::current_exe() {
            Ok(exe_path) => {
                println!("Path of this executable is: {}", exe_path.display());
                exe_path
            }
            Err(e) => panic!("failed to get current exe path: {e:?}"),
        };
        // calculate hash from executable
        let bytes = std::fs::read(pathfilename).unwrap(); // Vec<u8>
        let hash = sha256::digest(&bytes);
        VersionInformation {
            major: VERSION_MAJOR,
            minor: VERSION_MINOR,
            build: VERSION_BUILD,
            hash: hash,
        }
    }
}
