/*
 * meli
 *
 * Copyright 2017-2018 Manos Pitsidianakis
 *
 * This file is part of meli.
 *
 * meli is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * meli is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with meli. If not, see <http://www.gnu.org/licenses/>.
 */

use std::fs;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use uuid::Uuid;

enum FileType {
    Real,
    #[cfg(target_os = "linux")]
    Memory {
        fd: std::os::unix::io::RawFd,
    },
}

impl core::fmt::Debug for FileType {
    fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            FileType::Real => fmt.debug_struct("FileType::Real").finish(),
            #[cfg(target_os = "linux")]
            FileType::Memory { fd } => fmt
                .debug_struct(&format!("FileType::Memory({})", fd))
                .finish(),
        }
    }
}

#[derive(Debug)]
pub struct MeliFile {
    backing: FileType,
    pub path: PathBuf,
    delete_on_drop: bool,
}

impl Drop for MeliFile {
    fn drop(&mut self) {
        if self.delete_on_drop {
            std::fs::remove_file(self.path()).unwrap_or_else(|_| {});
        }
    }
}

impl MeliFile {
    pub fn get_file(&self) -> std::fs::File {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&self.path)
            .unwrap()
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn read_to_string(&self) -> String {
        let mut buf = Vec::new();
        let mut f = fs::File::open(&self.path)
            .unwrap_or_else(|_| panic!("Can't open {}", &self.path.display()));
        f.read_to_end(&mut buf)
            .unwrap_or_else(|_| panic!("Can't read {}", &self.path.display()));
        String::from_utf8(buf).unwrap()
    }

    /// Returned [`MeliFile`] will be deleted when dropped if delete_on_drop is set, so make sure to
    /// add it on [`Context'] `temp_files` to reap it later.
    pub fn create_temp_file(
        bytes: &[u8],
        filename: Option<&str>,
        path: Option<&PathBuf>,
        read_only: bool,
        delete_on_drop: bool,
    ) -> MeliFile {
        #[cfg(target_os = "linux")]
        if delete_on_drop && read_only && filename.is_none() && path.is_none() {
            debug!("creating memfd");
            match MeliFile::create_mem_file(bytes) {
                Ok(f) => return f,
                Err(err) => {
                    debug!("creating memfd failed {:?}", &err);
                    melib::log(
                        format!(
                            "Could not memfd_create file of len {}: {}",
                            bytes.len(),
                            err
                        ),
                        melib::LoggingLevel::DEBUG,
                    );
                }
            }
        }

        let mut dir = std::env::temp_dir();

        let path = path.unwrap_or_else(|| {
            dir.push("meli");
            std::fs::DirBuilder::new()
                .recursive(true)
                .create(&dir)
                .unwrap();
            if let Some(filename) = filename {
                dir.push(filename)
            } else {
                let u = Uuid::new_v4();
                dir.push(u.to_hyphenated().to_string());
            }
            &dir
        });

        let mut f = std::fs::File::create(path).unwrap();
        let metadata = f.metadata().unwrap();
        let mut permissions = metadata.permissions();

        permissions.set_mode(0o600); // Read/write for owner only.
        f.set_permissions(permissions).unwrap();

        f.write_all(bytes).unwrap();
        f.flush().unwrap();
        MeliFile {
            backing: FileType::Real,
            path: path.clone(),
            delete_on_drop,
        }
    }

    #[cfg(target_os = "linux")]
    pub fn create_mem_file(bytes: &[u8]) -> melib::Result<MeliFile> {
        use std::convert::TryInto;

        use nix::fcntl::SealFlag;
        use nix::sys::memfd::{memfd_create, MemFdCreateFlag};
        use std::ffi::CStr;
        let name: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(&b"meli\0"[..]) };
        let len = bytes
            .len()
            .try_into()
            .map_err(|err| Box::new(err) as Box<dyn std::error::Error + Sync + Send + 'static>)?;

        let fd = debug!(memfd_create(
            name,
            MemFdCreateFlag::MFD_ALLOW_SEALING //| MemFdCreateFlag::MFD_CLOEXEC,
        ))?;
        debug!(nix::unistd::ftruncate(fd, len))?;
        let addr = unsafe {
            debug!(nix::sys::mman::mmap(
                std::ptr::null_mut(),
                bytes.len(),
                nix::sys::mman::ProtFlags::PROT_WRITE,
                nix::sys::mman::MapFlags::MAP_SHARED,
                fd,
                0,
            ))?
        };
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), addr as *mut u8, bytes.len()) };
        debug!(unsafe { nix::sys::mman::munmap(addr, bytes.len()) })?;
        debug!(nix::fcntl::fcntl(
            fd,
            nix::fcntl::FcntlArg::F_ADD_SEALS(
                SealFlag::F_SEAL_SHRINK
                    | SealFlag::F_SEAL_GROW
                    | SealFlag::F_SEAL_WRITE
                    | SealFlag::F_SEAL_SEAL,
            ),
        ))?;
        Ok(MeliFile {
            backing: FileType::Memory { fd },
            path: PathBuf::from(format!(
                "/proc/{pid}/fd/{fd}",
                pid = std::process::id(),
                fd = fd
            )),
            delete_on_drop: true,
        })
    }
}
