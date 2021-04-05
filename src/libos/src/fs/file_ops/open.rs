use super::*;

pub fn do_openat(fs_path: &FsPath, flags: u32, mode: u32) -> Result<FileDesc> {
    debug!(
        "openat: fs_path: {:?}, flags: {:#o}, mode: {:#o}",
        fs_path, flags, mode
    );

    let path = fs_path.to_abs_path()?;
    let current = current!();
    let fs = current.fs().lock().unwrap();

    let sync_file: Arc<dyn SyncFile> = fs.open_file(&path, flags, mode)?;
    let file_ref = FileRef::from_sync(sync_file);

    let fd = {
        let creation_flags = CreationFlags::from_bits_truncate(flags);
        current.add_file(file_ref, creation_flags.must_close_on_spawn())
    };
    Ok(fd)
}
