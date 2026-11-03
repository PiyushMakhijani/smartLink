//! Windows implementation: symlinks, hardlinks (NTFS) and junction points.
//!
//! Detection reads the reparse tag via `GetFileInformationByHandleEx`
//! (`FileAttributeTagInfo`), which is the only reliable way to tell a
//! junction apart from a symlink. Junction creation uses
//! `FSCTL_SET_REPARSE_POINT` directly, mirroring `mklink /J` (both need
//! SeCreateSymbolicLinkPrivilege for regular users = Developer Mode).

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_ALL, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, CreateHardLinkW, FileAttributeTagInfo, GetFileInformationByHandle,
    GetFileInformationByHandleEx, GetVolumePathNameW, RemoveDirectoryW, BY_HANDLE_FILE_INFORMATION,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_INFO_BY_HANDLE_CLASS, OPEN_EXISTING,
};
use windows_sys::Win32::System::Ioctl::{
    FSCTL_DUPLICATE_EXTENTS_TO_FILE, FSCTL_SET_REPARSE_POINT, FSCTL_SET_SPARSE,
};
use windows_sys::Win32::System::SystemServices::{
    IO_REPARSE_TAG_MOUNT_POINT, IO_REPARSE_TAG_SYMLINK,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

use crate::link::{LinkInfo, LinkKind};

fn to_wide(path: &Path) -> Vec<u16> {
    let mut v: Vec<u16> = path.as_os_str().encode_wide().collect();
    v.push(0);
    v
}

unsafe fn tag_and_attr(handle: HANDLE) -> Option<(u32, u32)> {
    #[repr(C)]
    struct FileAttributeTagInfoStruct {
        file_attributes: u32,
        reparse_tag: u32,
    }
    let mut info = FileAttributeTagInfoStruct {
        file_attributes: 0,
        reparse_tag: 0,
    };
    let ok = GetFileInformationByHandleEx(
        handle,
        FileAttributeTagInfo,
        &mut info as *mut _ as *mut core::ffi::c_void,
        std::mem::size_of::<FileAttributeTagInfoStruct>() as u32,
    );
    if ok != 0 {
        Some((info.reparse_tag, info.file_attributes))
    } else {
        None
    }
}

unsafe fn links_and_ids(handle: HANDLE) -> Option<(u32, u64)> {
    let mut info: BY_HANDLE_FILE_INFORMATION = std::mem::zeroed();
    if GetFileInformationByHandle(handle, &mut info) != 0 {
        let id = ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64;
        Some((info.nNumberOfLinks, id))
    } else {
        None
    }
}

unsafe fn open_raw(
    path: &Path,
    access: u32,
    share: u32,
    disposition: u32,
    flags: u32,
) -> Option<HANDLE> {
    let handle = CreateFileW(
        to_wide(path).as_ptr(),
        access,
        share,
        core::ptr::null(),
        disposition,
        flags,
        INVALID_HANDLE_VALUE,
    );
    if handle == INVALID_HANDLE_VALUE {
        None
    } else {
        Some(handle)
    }
}

unsafe fn open(path: &Path, flags: u32) -> Result<HANDLE> {
    open_raw(
        path,
        GENERIC_READ,
        0x3, /* READ|WRITE share */
        OPEN_EXISTING,
        flags,
    )
    .ok_or_else(|| anyhow!(io::Error::last_os_error()))
}

/// Detect a symlink or junction at `path` (hardlinks are not reparse points,
/// they are reported through `hardlink_count` / `file_id` instead).
pub fn detect(path: &Path) -> Result<Option<LinkInfo>> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => return Ok(None),
    };
    let is_link = meta.is_symlink()
        || unsafe {
            open(
                path,
                FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            )
            .ok()
            .map(|h| {
                let out = tag_and_attr(h)
                    .map(|(tag, attr)| {
                        attr & FILE_ATTRIBUTE_REPARSE_POINT != 0
                            && (tag == IO_REPARSE_TAG_MOUNT_POINT || tag == IO_REPARSE_TAG_SYMLINK)
                    })
                    .unwrap_or(false);
                CloseHandle(h);
                out
            })
            .unwrap_or(false)
        };
    if !is_link {
        return Ok(None);
    }

    let tag = unsafe {
        let h = open(
            path,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
        )?;
        let t = tag_and_attr(h).map(|(tag, _)| tag);
        CloseHandle(h);
        t
    };
    let kind = match tag {
        Some(IO_REPARSE_TAG_MOUNT_POINT) => LinkKind::Junction,
        _ => LinkKind::Symlink, // std says is_symlink and tag is symlink/unknown-but-link
    };

    // std::fs::read_link works for both symlinks and junctions on Windows.
    let target = std::fs::read_link(path).unwrap_or_default();
    let resolved = if target.is_absolute() {
        target.clone()
    } else {
        crate::fs::link_base_dir(path).join(&target)
    };

    Ok(Some(LinkInfo {
        path: path.to_path_buf(),
        kind,
        target,
        alive: Path::new(&resolved).exists(),
        nlinks: hardlink_count(path).unwrap_or(1),
        file_id: file_id(path).unwrap_or(None),
    }))
}

/// NTFS `NumberOfLinks` for the file.
pub fn hardlink_count(path: &Path) -> Result<u64> {
    unsafe {
        let h = open(path, FILE_FLAG_BACKUP_SEMANTICS)?;
        let out = links_and_ids(h).map(|(n, _)| n as u64);
        CloseHandle(h);
        out.context("could not read link count")
    }
}

/// 64-bit view of the 128-bit NTFS file index, used to group hardlinks.
pub fn file_id(path: &Path) -> Result<Option<u64>> {
    unsafe {
        let h = match open_raw(
            path,
            GENERIC_READ,
            0x3,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
        ) {
            Some(h) => h,
            None => return Ok(None),
        };
        let out = links_and_ids(h).map(|(_, id)| id);
        CloseHandle(h);
        Ok(out)
    }
}

fn volume_of(path: &Path) -> Option<String> {
    let mut buf = vec![0u16; 260];
    let ok =
        unsafe { GetVolumePathNameW(to_wide(path).as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
    if ok != 0 {
        let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]).to_uppercase())
    } else {
        root_prefix(path)
    }
}

fn root_prefix(path: &Path) -> Option<String> {
    let s = path.as_os_str().to_string_lossy().to_string();
    let chars: Vec<char> = s.chars().collect();
    if chars.len() >= 2 && chars[1] == ':' && chars[0].is_ascii_alphabetic() {
        Some(chars[0].to_ascii_uppercase().to_string())
    } else if let Some(rest) = s.strip_prefix(r"\\") {
        // UNC: server\share is the volume-equivalent prefix
        Some(
            rest.split(['/', '\\'])
                .take(2)
                .collect::<Vec<_>>()
                .join(r"\"),
        )
    } else {
        None
    }
}

/// Compare volume roots (drive letters or UNC shares).
pub fn same_volume(a: &Path, b: &Path) -> bool {
    match (volume_of(a), volume_of(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// `mklink /H` equivalent.
pub fn create_hardlink(target: &Path, link: &Path) -> Result<()> {
    let ok = unsafe {
        CreateHardLinkW(
            to_wide(link).as_ptr(),
            to_wide(target).as_ptr(),
            core::ptr::null(),
        )
    };
    if ok == 0 {
        return Err(anyhow!(io::Error::last_os_error())).with_context(|| {
            format!(
                "hardlink {} -> {} failed (NTFS only, same volume, files only)",
                link.display(),
                target.display()
            )
        });
    }
    Ok(())
}

fn create_symlink(target: &Path, link: &Path, dir: bool) -> Result<()> {
    let result = if dir {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    };
    result.map_err(|e| {
        if e.raw_os_error() == Some(1314) {
            anyhow!(
                "symlink {} -> {} failed: missing privilege. Enable Windows Developer \
                 Mode or run elevated, or use --junction for directories.",
                link.display(),
                target.display()
            )
        } else {
            anyhow!(e)
        }
    })
}

/// `mklink` equivalent for files (needs Developer Mode or admin rights).
pub fn create_symlink_file(target: &Path, link: &Path) -> Result<()> {
    create_symlink(target, link, false)
}

/// `mklink` equivalent for directories.
pub fn create_symlink_dir(target: &Path, link: &Path) -> Result<()> {
    create_symlink(target, link, true)
}

/// `mklink /J` equivalent: writes a mount-point reparse buffer onto a fresh
/// (empty) directory. Needs the SeCreateSymbolicLinkPrivilege that regular
/// (non-admin) Windows 10/11 users get via Developer Mode, same as `mklink /J`.
/// Targets are always absolute, so `--relative` cannot be honoured for junctions.
pub fn create_junction(target: &Path, link: &Path) -> Result<()> {
    let abs = target
        .canonicalize()
        .with_context(|| format!("junction target {} must exist", target.display()))?;
    let abs = strip_verbatim_prefix(&abs);

    // REPARSE_DATA_BUFFER / MountPointReparseBuffer layout (mirrors the
    // `junction` crate's proven construction):
    //   PathBuffer = substitute ("\\??\\" + target, no NUL in length)
    //              + NUL + print (target, no NUL in length) + NUL
    let prefix: Vec<u16> = std::ffi::OsStr::new("\\??\\").encode_wide().collect();
    let target_w: Vec<u16> = abs.as_os_str().encode_wide().collect();
    let substitute_bytes = ((prefix.len() + target_w.len()) * 2) as u16;
    let print_bytes = (target_w.len() * 2) as u16;
    let reparse_data_length = 8 + substitute_bytes + 2 + print_bytes + 2;
    let path_buffer_len = (substitute_bytes as usize / 2) + 1 + (print_bytes as usize / 2) + 1;

    #[repr(C)]
    #[allow(dead_code)] // raw REPARSE_DATA_BUFFER mirror
    struct MountPointHeader {
        reparse_tag: u32,
        reparse_data_length: u16,
        reserved: u16,
        substitute_name_offset: u16,
        substitute_name_length: u16,
        print_name_offset: u16,
        print_name_length: u16,
    }

    let hdr_len = std::mem::size_of::<MountPointHeader>(); // 16
    let mut out = vec![0u8; hdr_len + path_buffer_len * 2];
    let buf = out.as_mut_ptr() as *mut MountPointHeader;
    unsafe {
        (*buf).reparse_tag = IO_REPARSE_TAG_MOUNT_POINT;
        (*buf).reparse_data_length = reparse_data_length;
        (*buf).reserved = 0;
        (*buf).substitute_name_offset = 0;
        (*buf).substitute_name_length = substitute_bytes;
        (*buf).print_name_offset = substitute_bytes + 2;
        (*buf).print_name_length = print_bytes;
        let names = out.as_mut_ptr().add(hdr_len) as *mut u16;
        let mut w = names;
        std::ptr::copy_nonoverlapping(prefix.as_ptr(), w, prefix.len());
        w = w.add(prefix.len());
        std::ptr::copy_nonoverlapping(target_w.as_ptr(), w, target_w.len());
        w = w.add(target_w.len());
        *w = 0;
        w = w.add(1);
        std::ptr::copy_nonoverlapping(target_w.as_ptr(), w, target_w.len());
        w = w.add(target_w.len());
        *w = 0;
    }
    // in_buffer_size = header (8) + ReparseDataLength
    let in_buffer_size = (8 + reparse_data_length) as u32;

    // The link path must exist as an empty directory before setting the tag.
    if !link.exists() {
        std::fs::create_dir_all(link)
            .with_context(|| format!("could not create junction placeholder {}", link.display()))?;
    }

    let handle = unsafe {
        open_raw(
            link,
            GENERIC_READ | GENERIC_WRITE,
            0,
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
        )
        .with_context(|| format!("could not open {} for reparse write", link.display()))?
    };
    let mut returned: u32 = 0;
    let ok = unsafe {
        DeviceIoControl(
            handle,
            FSCTL_SET_REPARSE_POINT,
            out.as_mut_ptr() as *mut core::ffi::c_void,
            in_buffer_size,
            core::ptr::null_mut(),
            0,
            &mut returned,
            core::ptr::null_mut(),
        )
    };
    unsafe { CloseHandle(handle) };
    if ok == 0 {
        let err = io::Error::last_os_error();
        let _ = std::fs::remove_dir(link); // roll back the placeholder
        return Err(anyhow!(err)).with_context(|| {
            format!(
                "could not create junction {} -> {}",
                link.display(),
                abs.display()
            )
        });
    }
    Ok(())
}

/// `canonicalize` yields `\\?\C:\...`; junction targets must be plain paths.
fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    let s = path.as_os_str().to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest.to_string()),
        None => path.to_path_buf(),
    }
}

/// True when `path` is an NTFS junction (mount-point reparse point), as
/// opposed to a real directory or a symlink.
fn is_junction(path: &Path) -> bool {
    unsafe {
        match open(
            path,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
        ) {
            Ok(h) => {
                let out = tag_and_attr(h)
                    .map(|(tag, attr)| {
                        attr & FILE_ATTRIBUTE_REPARSE_POINT != 0
                            && tag == IO_REPARSE_TAG_MOUNT_POINT
                    })
                    .unwrap_or(false);
                CloseHandle(h);
                out
            }
            Err(_) => false,
        }
    }
}

/// Delete whatever `path` is, target-safe.
///
/// * Junctions (mount-point reparse points) can't go through
///   `std::fs::remove_dir` (it opens/validates the target and returns
///   ERROR_ACCESS_DENIED); `RemoveDirectoryW` on the reparse point is the
///   exact reverse of `mklink /J` and never touches the target contents.
/// * A *directory* symlink must be removed with the directory API -
///   `std::fs::remove_file` (DeleteFileW) fails on it with ERROR_ACCESS_DENIED -
///   while a *file* symlink needs the file API. The target may be dead (so we
///   can't stat it to learn the type), therefore we try one and fall back to the
///   other, reporting the last error only if both fail.
/// * Real files/hardlinks and plain directories use the matching std call.
pub fn remove(path: &Path) -> Result<()> {
    if is_junction(path) {
        let ok = unsafe { RemoveDirectoryW(to_wide(path).as_ptr()) };
        if ok == 0 {
            return Err(anyhow!(io::Error::last_os_error()))
                .with_context(|| format!("could not remove junction {}", path.display()));
        }
        return Ok(());
    }
    let meta = std::fs::symlink_metadata(path)
        .with_context(|| format!("could not stat {}", path.display()))?;
    if meta.is_symlink() {
        // Order-independent: try file delete, fall back to directory delete.
        if std::fs::remove_file(path).is_ok() {
            return Ok(());
        }
        std::fs::remove_dir(path)
            .with_context(|| format!("could not remove symlink {}", path.display()))
    } else if meta.is_dir() {
        std::fs::remove_dir(path)
            .with_context(|| format!("could not remove directory {}", path.display()))
    } else {
        std::fs::remove_file(path).with_context(|| format!("could not remove {}", path.display()))
    }
}

/// ReFS/NTFS `FSCTL_DUPLICATE_EXTENTS_TO_FILE` chunk loop (dedupe backend).
/// Opens `dest` fresh (creating it), marks it sparse and duplicates the whole
/// extent chain of `src`. Unsupported layouts (plain NTFS < Windows 11 24H2,
/// ReFS unavailable, misaligned small files) surface as an error instead of a
/// silent plain copy.
/// SAFETY: manual FILE_DUPLICATE_EXTENTS equivalent (u64/u32 fields, 24 bytes).
#[repr(C)]
struct FileDuplicateExtents {
    source_file: HANDLE,
    source_file_offset: u64,
    length: u32,
    _reserved: u32,
    _reserved2: u64,
}

pub fn try_duplicate_extents(src: &Path, dest: &Path) -> Result<bool> {
    let src_handle = unsafe {
        open_raw(
            src,
            GENERIC_READ,
            0x3,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
        )
        .with_context(|| format!("could not open source {}", src.display()))?
    };
    let dest_handle = unsafe {
        open_raw(
            dest,
            GENERIC_ALL,
            0,
            2, /* CREATE_ALWAYS */
            FILE_FLAG_BACKUP_SEMANTICS,
        )
        .with_context(|| format!("could not create dest {}", dest.display()))?
    };
    let result = (|| -> Result<bool> {
        let mut returned: u32 = 0;
        unsafe {
            // ReFS duplicates extents into sparse files; the call errors on
            // layouts where CoW is not permitted.
            if windows_sys::Win32::System::IO::DeviceIoControl(
                dest_handle,
                FSCTL_SET_SPARSE,
                core::ptr::null_mut(),
                0,
                core::ptr::null_mut(),
                0,
                &mut returned,
                core::ptr::null_mut(),
            ) == 0
            {
                return Ok(false);
            }
        }
        let len = std::fs::metadata(src)?.len();
        const CHUNK: u64 = 1 << 30; // stay below the u32 Length field
        let mut offset: u64 = 0;
        while offset < len {
            let n = (len - offset).min(CHUNK) as u32;
            let req = FileDuplicateExtents {
                source_file: src_handle,
                source_file_offset: offset,
                length: n,
                _reserved: 0,
                _reserved2: 0,
            };
            let ok = unsafe {
                windows_sys::Win32::System::IO::DeviceIoControl(
                    dest_handle,
                    FSCTL_DUPLICATE_EXTENTS_TO_FILE,
                    &req as *const _ as *const core::ffi::c_void,
                    std::mem::size_of::<FileDuplicateExtents>() as u32,
                    core::ptr::null_mut(),
                    0,
                    &mut returned,
                    core::ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Ok(false);
            }
            offset += n as u64;
        }
        // Duplicate extents do not extend the end-of-file; fix the size.
        if len > 0 {
            unsafe {
                windows_sys::Win32::Storage::FileSystem::SetFilePointerEx(
                    dest_handle,
                    len as i64,
                    core::ptr::null_mut(),
                    0, /* FILE_BEGIN */
                )
            };
            unsafe { windows_sys::Win32::Storage::FileSystem::SetEndOfFile(dest_handle) };
        }
        Ok(true)
    })();
    unsafe {
        CloseHandle(src_handle);
        CloseHandle(dest_handle);
    }
    result
}

// FILE_INFO_BY_HANDLE_CLASS is an i32 alias in windows-sys; keep the import
// explicit so version drift shows up at compile time.
#[allow(dead_code)]
type _InfoClass = FILE_INFO_BY_HANDLE_CLASS;
