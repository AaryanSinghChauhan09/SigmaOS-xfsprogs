// SPDX-License-Identifier: MIT
// SigmaOS-xfsprogs - XFS Filesystem Utilities component
// xfsprogs-style tools for XFS filesystem administration in SigmaOS,
// inspired by Arch's xfsprogs package (mkfs.xfs, xfs_repair, xfs_info, etc.)

use std::fmt;

// ============================================================================
// XFS GEOMETRY
// ============================================================================

/// XFS allocation group geometry
#[derive(Debug, Clone, Copy)]
pub struct AgGeometry {
    /// Allocation group size in bytes
    pub ag_size: u64,
    /// Number of allocation groups
    pub ag_count: u32,
    /// AG alignment in bytes
    pub ag_align: u64,
}

impl AgGeometry {
    /// Compute AG geometry for a given device size
    /// XFS uses ~4 allocation groups minimum, AG size is device_size / ag_count
    pub fn for_device_size(size_bytes: u64) -> Self {
        // AG size must be multiple of filesystem block size (typically 4K)
        let block = 4096u64;
        // XFS prefers AG count to make AG size reasonable (~16-64GB per AG)
        let target_ag_size = 16 * 1024 * 1024 * 1024u64; // 16 GiB
        let mut ag_count = (size_bytes / target_ag_size).max(1) as u32;
        // Cap at reasonable max
        ag_count = ag_count.min(32);
        let mut ag_size = (size_bytes / ag_count as u64) & !(block - 1);
        if ag_size < block {
            ag_size = block;
            ag_count = 1;
        }
        Self {
            ag_size,
            ag_count,
            ag_align: block,
        }
    }
}

/// XFS superblock information
#[derive(Debug, Clone)]
pub struct XfsSuperblock {
    pub magic: u32,              // 0x58465342 "XFSB"
    pub block_size: u32,         // 512, 1024, 2048, 4096, 8192
    pub ag_count: u32,
    pub ag_size: u64,
    pub total_blocks: u64,
    pub free_blocks: u64,
    pub log_start_block: u64,    // journal location
    pub log_blocks: u64,
    pub realtime_start: u64,
    pub realtime_blocks: u64,
    pub uuid: String,
    pub label: String,
    pub version_num: u32,
    pub features_compat: u64,
    pub features_ro_compat: u64,
    pub features_incompat: u64,
    pub inode_size: u16,
    pub dir_block_size: u16,
    pub sector_size: u16,
    pub dblocks_log: u8,
    pub rblocks_log: u8,
    pub rextents_log: u8,
}

impl XfsSuperblock {
    pub const MAGIC: u32 = 0x5846_5342; // "XFSB"

    pub fn new(block_size: u32, total_bytes: u64) -> Self {
        let geom = AgGeometry::for_device_size(total_bytes);
        let total_blocks = total_bytes / block_size as u64;
        let log_blocks = (total_blocks / 100).max(64); // ~1% journal
        Self {
            magic: Self::MAGIC,
            block_size,
            ag_count: geom.ag_count,
            ag_size: geom.ag_size,
            total_blocks,
            free_blocks: total_blocks * 95 / 100, // ~5% reserved
            log_start_block: 0,
            log_blocks,
            realtime_start: total_blocks,
            realtime_blocks: 0,
            uuid: String::new(),
            label: String::new(),
            version_num: 0,
            features_compat: 0,
            features_ro_compat: 0,
            features_incompat: 0,
            inode_size: 256,
            dir_block_size: 4096,
            sector_size: 512,
            dblocks_log: 0,
            rblocks_log: 0,
            rextents_log: 0,
        }
    }

    pub fn used_blocks(&self) -> u64 {
        self.total_blocks.saturating_sub(self.free_blocks)
    }

    pub fn usage_percent(&self) -> f64 {
        if self.total_blocks == 0 {
            return 0.0;
        }
        self.used_blocks() as f64 / self.total_blocks as f64 * 100.0
    }

    pub fn free_bytes(&self) -> u64 {
        self.free_blocks * self.block_size as u64
    }

    /// Whether the superblock looks valid
    pub fn is_valid(&self) -> bool {
        self.magic == Self::MAGIC
            && self.block_size >= 512
            && self.block_size <= 65536
            && self.block_size.is_power_of_two()
            && self.ag_count > 0
            && self.ag_size >= self.block_size as u64
    }
}

// ============================================================================
// XFS FEATURE FLAGS
// ============================================================================

/// XFS feature flag bits (subset)
pub mod features {
    pub const FINOMAP: u64 = 0x0000_0001;        // inode level extents
    pub const EXTWIDTH: u64 = 0x0000_0002;       // large extent counters
    pub const ASANAME: u64 = 0x0000_0004;        // attr in shortform
    pub const PROJID32BIT: u64 = 0x0000_0008;    // 32-bit project IDs
    pub const SPARSE_INODES: u64 = 0x0000_0010;  // sparse inode chunks
    pub const METADIRV2: u64 = 0x0000_0020;       // metadata dir v2
    pub const LAZYSB_COUNT: u64 = 0x0000_0040;   // lazy sb counters
    pub const BIGTIME: u64 = 0x0000_0080;        // big timestamps
    pub const V3INOPS: u64 = 0x0000_0100;        // v3 inodes
    pub const CRC: u64 = 0x0000_0200;            // CRC32C checksums
    pub const SPINODES: u64 = 0x0000_0400;       // sparse inode chunks
    pub const METADIRV2_INO32: u64 = 0x0000_0800;
    pub const NREXT64: u64 = 0x0000_1000;        // 64-bit extent counts
    pub const EXCHANGE_RANGE: u64 = 0x0000_2000;
}

/// Human-readable feature names
pub fn feature_names(flags: u64) -> Vec<&'static str> {
    let mut names = Vec::new();
    if flags & features::FINOMAP != 0 { names.push("finomap"); }
    if flags & features::EXTWIDTH != 0 { names.push("extwidth"); }
    if flags & features::ASANAME != 0 { names.push("asaname"); }
    if flags & features::PROJID32BIT != 0 { names.push("projid32bit"); }
    if flags & features::SPARSE_INODES != 0 { names.push("sparse-inodes"); }
    if flags & features::METADIRV2 != 0 { names.push("metadirv2"); }
    if flags & features::LAZYSB_COUNT != 0 { names.push("lazysbcount"); }
    if flags & features::BIGTIME != 0 { names.push("bigtime"); }
    if flags & features::V3INOPS != 0 { names.push("v3inops"); }
    if flags & features::CRC != 0 { names.push("crc"); }
    if flags & features::SPINODES != 0 { names.push("sparse-inodes"); }
    if flags & features::METADIRV2_INO32 != 0 { names.push("metadirv2-ino32"); }
    if flags & features::NREXT64 != 0 { names.push("nrext64"); }
    if flags & features::EXCHANGE_RANGE != 0 { names.push("exchange-range"); }
    names
}

// ============================================================================
// XFS INODE
// ============================================================================

/// XFS inode modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InodeMode {
    /// Regular file
    File,
    /// Directory
    Directory,
    /// Symlink
    Symlink,
    /// Special file (device, fifo, socket)
    Special,
}

/// An XFS inode record
#[derive(Debug, Clone)]
pub struct XfsInode {
    /// Inode number
    pub ino: u64,
    /// Inode mode (file type + permissions)
    pub mode: u16,
    /// Owner UID
    pub uid: u32,
    /// Owner GID
    pub gid: u32,
    /// File size in bytes
    pub size: u64,
    /// Number of data blocks allocated
    pub blocks: u64,
    /// Access time (seconds since epoch)
    pub atime: i64,
    /// Modification time
    pub mtime: i64,
    /// Change time
    pub ctime: i64,
    /// Block device numbers for special files
    pub rdev: Option<(u32, u32)>,
    /// Number of links
    pub nlink: u32,
    /// Flags
    pub flags: u16,
}

impl XfsInode {
    pub fn is_dir(&self) -> bool {
        (self.mode & 0o170000) == 0o040000
    }
    pub fn is_file(&self) -> bool {
        (self.mode & 0o170000) == 0o100000
    }
    pub fn is_symlink(&self) -> bool {
        (self.mode & 0o170000) == 0o120000
    }
    pub fn permissions(&self) -> u32 {
        (self.mode & 0o7777) as u32
    }
}

// ============================================================================
// XFS UTILITIES
// ============================================================================

/// Result of filesystem check (xfs_repair-style)
#[derive(Debug, Clone)]
pub struct FsckResult {
    /// Whether filesystem is clean
    pub clean: bool,
    /// Number of errors found
    pub errors: u32,
    /// Number of warnings
    pub warnings: u32,
    /// List of issues found
    pub issues: Vec<String>,
}

/// Run a simulated filesystem check
pub fn check_filesystem(sb: &XfsSuperblock) -> FsckResult {
    let mut issues = Vec::new();
    let mut errors = 0u32;
    let mut warnings = 0u32;

    if !sb.is_valid() {
        errors += 1;
        issues.push("invalid superblock magic".into());
    }
    if sb.free_blocks > sb.total_blocks {
        errors += 1;
        issues.push("free blocks exceed total blocks".into());
    }
    if sb.log_blocks > sb.total_blocks / 10 {
        warnings += 1;
        issues.push("journal size unusually large".into());
    }
    if sb.inode_size < 128 || sb.inode_size > 1024 {
        warnings += 1;
        issues.push(format!("unusual inode size: {}", sb.inode_size));
    }
    if sb.block_size < 4096 {
        warnings += 1;
        issues.push(format!("small block size: {}", sb.block_size));
    }

    FsckResult {
        clean: errors == 0,
        errors,
        warnings,
        issues,
    }
}

/// Estimate optimal block size for a filesystem
pub fn recommended_block_size(total_bytes: u64) -> u32 {
    if total_bytes < 300 * 1024 * 1024 {
        4096 // small fs: 4K blocks
    } else if total_bytes < 10 * 1024 * 1024 * 1024 {
        4096
    } else {
        4096 // XFS default is always 4K in practice
    }
}

/// Estimate optimal inode size
pub fn recommended_inode_size(has_large_files: bool, has_many_files: bool) -> u16 {
    if has_large_files || has_many_files {
        512 // larger inodes for extents/xattrs
    } else {
        256 // default
    }
}

// ============================================================================
// XFS TOOL COMMANDS
// ============================================================================

/// Command types for XFS utilities
#[derive(Debug, Clone)]
pub enum XfsCommand {
    /// mkfs.xfs - create filesystem
    Mkfs {
        device: String,
        size_bytes: u64,
        block_size: u32,
        label: Option<String>,
        force: bool,
    },
    /// xfs_repair - check/repair filesystem
    Repair {
        device: String,
        dry_run: bool,
        aggressive: bool,
    },
    /// xfs_info - display filesystem info
    Info { device: String },
    /// xfs_growfs - grow filesystem
    GrowFs { mountpoint: String, new_size_bytes: u64 },
    /// xfs_quota - manage quotas
    Quota { mountpoint: String, user: Option<String>, limit_bytes: u64 },
}

/// Build mkfs.xfs command args
pub fn mkfs_args(cmd: &XfsCommand) -> Vec<String> {
    match cmd {
        XfsCommand::Mkfs { device, block_size, label, force, .. } => {
            let mut args = vec!["mkfs.xfs".to_string()];
            args.push(format!("-b{}", block_size));
            if let Some(l) = label {
                args.push(format!("-L{}", l));
            }
            if *force {
                args.push("-f".to_string());
            }
            args.push(device.clone());
            args
        }
        XfsCommand::Repair { device, dry_run, aggressive } => {
            let mut args = vec!["xfs_repair".to_string()];
            if *dry_run { args.push("-n".to_string()); }
            if *aggressive { args.push("-v".to_string()); }
            args.push(device.clone());
            args
        }
        XfsCommand::Info { device } => {
            vec!["xfs_info".to_string(), device.clone()]
        }
        XfsCommand::GrowFs { mountpoint, new_size_bytes } => {
            vec![
                "xfs_growfs".to_string(),
                format!("-D{}", new_size_bytes),
                mountpoint.clone(),
            ]
        }
        XfsCommand::Quota { mountpoint, user, limit_bytes } => {
            let mut args = vec!["xfs_quota".to_string(), "-x".to_string()];
            if let Some(u) = user {
                args.push(format!("-u{}", u));
            }
            args.push("limit".to_string());
            args.push(format!("-b{}", limit_bytes));
            args.push(mountpoint.clone());
            args
        }
    }
}

// ============================================================================
// ERRORS
// ============================================================================

#[derive(Debug)]
pub enum XfsError {
    InvalidSuperblock,
    DeviceTooSmall(u64),
    DeviceNotFound(String),
    RepairFailed(String),
    NotXfsFilesystem,
}

impl fmt::Display for XfsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSuperblock => write!(f, "invalid XFS superblock"),
            Self::DeviceTooSmall(size) => write!(f, "device too small: {} bytes", size),
            Self::DeviceNotFound(d) => write!(f, "device not found: {}", d),
            Self::RepairFailed(m) => write!(f, "repair failed: {}", m),
            Self::NotXfsFilesystem => write!(f, "not an XFS filesystem"),
        }
    }
}
impl std::error::Error for XfsError {}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ag_geometry() {
        let g = AgGeometry::for_device_size(100 * 1024 * 1024 * 1024); // 100GB
        assert!(g.ag_count >= 1);
        assert!(g.ag_size >= 4096);
        assert_eq!(g.ag_size % 4096, 0);
    }

    #[test]
    fn test_superblock_valid() {
        let sb = XfsSuperblock::new(4096, 100 * 1024 * 1024 * 1024);
        assert!(sb.is_valid());
        assert_eq!(sb.magic, XfsSuperblock::MAGIC);
        assert!(sb.usage_percent() < 10.0); // mostly free
    }

    #[test]
    fn test_superblock_invalid() {
        let mut sb = XfsSuperblock::new(4096, 1024 * 1024);
        sb.magic = 0;
        assert!(!sb.is_valid());
    }

    #[test]
    fn test_feature_names() {
        let names = feature_names(features::CRC | features::FINOMAP);
        assert!(names.contains(&"crc"));
        assert!(names.contains(&"finomap"));
        assert!(!names.contains(&"nrext64"));
    }

    #[test]
    fn test_inode_types() {
        let file = XfsInode {
            ino: 1, mode: 0o100644, uid: 0, gid: 0, size: 0, blocks: 0,
            atime: 0, mtime: 0, ctime: 0, rdev: None, nlink: 1, flags: 0,
        };
        assert!(file.is_file());
        assert!(!file.is_dir());
        assert_eq!(file.permissions(), 0o644);
    }

    #[test]
    fn test_fsck_clean() {
        let sb = XfsSuperblock::new(4096, 100 * 1024 * 1024 * 1024);
        let result = check_filesystem(&sb);
        assert!(result.clean);
        assert_eq!(result.errors, 0);
    }

    #[test]
    fn test_fsck_detects_issues() {
        let mut sb = XfsSuperblock::new(4096, 100 * 1024 * 1024 * 1024);
        sb.free_blocks = sb.total_blocks + 1; // impossible
        sb.magic = 0;
        let result = check_filesystem(&sb);
        assert!(!result.clean);
        assert!(result.errors >= 2);
    }

    #[test]
    fn test_mkfs_args() {
        let cmd = XfsCommand::Mkfs {
            device: "/dev/sda1".into(),
            size_bytes: 1_000_000,
            block_size: 4096,
            label: Some("data".into()),
            force: true,
        };
        let args = mkfs_args(&cmd);
        assert!(args.contains(&"mkfs.xfs".to_string()));
        assert!(args.contains(&"-b4096".to_string()));
        assert!(args.contains(&"-Ldata".to_string()));
        assert!(args.contains(&"-f".to_string()));
        assert!(args.contains(&"/dev/sda1".to_string()));
    }

    #[test]
    fn test_repair_args() {
        let cmd = XfsCommand::Repair {
            device: "/dev/sda1".into(),
            dry_run: true,
            aggressive: false,
        };
        let args = mkfs_args(&cmd);
        assert!(args.contains(&"xfs_repair".to_string()));
        assert!(args.contains(&"-n".to_string()));
    }

    #[test]
    fn test_recommended_sizes() {
        assert_eq!(recommended_block_size(100 * 1024 * 1024), 4096);
        assert_eq!(recommended_inode_size(true, false), 512);
        assert_eq!(recommended_inode_size(false, false), 256);
    }
}
