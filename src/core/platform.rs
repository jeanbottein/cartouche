//! Current OS/arch tags and executable header parsing (ELF / PE / Mach-O).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Canonical OS tag stored in `game.json` targets.
pub fn os_tag() -> &'static str {
    match std::env::consts::OS {
        "linux" => "linux",
        "windows" => "windows",
        "macos" => "macos",
        _ => "other",
    }
}

/// Canonical arch tag; 32-bit x86 counts as x64 (x64 covers legacy x86).
pub fn arch_tag() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" | "arm" => "arm64",
        "x86_64" | "x86" => "x64",
        _ => "other",
    }
}

fn elf_arch(machine: u16) -> Option<&'static str> {
    match machine {
        0x03 | 0x3E => Some("x64"),
        0xB7 | 0x28 => Some("arm64"),
        _ => None,
    }
}

fn pe_arch(machine: u16) -> Option<&'static str> {
    match machine {
        0x014C | 0x8664 => Some("x64"),
        0xAA64 => Some("arm64"),
        _ => None,
    }
}

fn macho_arch(cpu: u32) -> Option<&'static str> {
    match cpu {
        7 | 0x0100_0007 => Some("x64"),
        12 | 0x0100_000C => Some("arm64"),
        _ => None,
    }
}

const MACHO_MAGICS: [u32; 2] = [0xFEED_FACE, 0xFEED_FACF];
const FAT_MAGIC: u32 = 0xCAFE_BABE;

fn u16_le(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn u32_be(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(b[at..at + 4].try_into().unwrap())
}

fn read_header(file: &mut File) -> Vec<u8> {
    let mut header = Vec::with_capacity(64);
    let _ = file.take(64).read_to_end(&mut header);
    header
}

fn read_pe_arch(file: &mut File, header: &[u8]) -> Option<&'static str> {
    if header.len() < 64 {
        return None;
    }
    file.seek(SeekFrom::Start(u32_le(header, 60) as u64)).ok()?;
    let mut sig = [0u8; 6];
    file.read_exact(&mut sig).ok()?;
    if &sig[..4] != b"PE\0\0" {
        return None;
    }
    pe_arch(u16_le(&sig, 4))
}

fn read_macho_arch(header: &[u8]) -> Option<&'static str> {
    if header.len() < 8 {
        return None;
    }
    if MACHO_MAGICS.contains(&u32_le(header, 0)) {
        return macho_arch(u32_le(header, 4));
    }
    if MACHO_MAGICS.contains(&u32_be(header, 0)) {
        return macho_arch(u32_be(header, 4));
    }
    None
}

/// Architecture of a binary from its header, or `None` if unrecognised.
pub fn detect_binary_arch(path: &Path) -> Option<&'static str> {
    let mut file = File::open(path).ok()?;
    let header = read_header(&mut file);
    if header.len() < 4 {
        return None;
    }
    if header.starts_with(b"\x7fELF") {
        return (header.len() >= 20)
            .then(|| elf_arch(u16_le(&header, 18)))
            .flatten();
    }
    if header.starts_with(b"MZ") {
        return read_pe_arch(&mut file, &header);
    }
    read_macho_arch(&header)
}

/// True for ELF, PE or Mach-O files (by magic bytes, not permissions).
pub fn is_executable(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut magic = Vec::with_capacity(4);
    let _ = (&mut file).take(4).read_to_end(&mut magic);
    if magic.len() < 2 {
        return false;
    }
    if magic.starts_with(b"\x7fELF") || magic.starts_with(b"MZ") {
        return true;
    }
    if magic.len() < 4 {
        return false;
    }
    let known = |v: u32| MACHO_MAGICS.contains(&v) || v == FAT_MAGIC;
    known(u32_le(&magic, 0)) || known(u32_be(&magic, 0))
}

/// Has an execute permission bit (always true where permissions don't exist).
pub fn has_exec_permission(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::testutil::TempDir;

    #[test]
    fn detects_elf_x64_and_pe_arm64() {
        let dir = TempDir::new();
        let mut elf = b"\x7fELF".to_vec();
        elf.resize(64, 0);
        elf[18] = 0x3E;
        let elf_path = dir.write("game", &elf);
        assert_eq!(detect_binary_arch(&elf_path), Some("x64"));
        assert!(is_executable(&elf_path));

        let mut pe = b"MZ".to_vec();
        pe.resize(64, 0);
        pe[60] = 64;
        pe.extend_from_slice(b"PE\0\0");
        pe.extend_from_slice(&0xAA64u16.to_le_bytes());
        let pe_path = dir.write("game.exe", &pe);
        assert_eq!(detect_binary_arch(&pe_path), Some("arm64"));
    }

    #[test]
    fn text_files_are_not_executables() {
        let dir = TempDir::new();
        let path = dir.write("readme.txt", b"hello world");
        assert!(!is_executable(&path));
        assert_eq!(detect_binary_arch(&path), None);
    }
}
