use std::path::PathBuf;

use crate::file_ops::TransferMode;

#[derive(Clone, Debug)]
pub struct ClipboardPayload {
    pub paths: Vec<PathBuf>,
    pub mode: TransferMode,
}

#[cfg(windows)]
mod platform {
    use std::ffi::OsString;
    use std::mem::size_of;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::PathBuf;
    use std::ptr;
    use std::thread;
    use std::time::Duration;

    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{GlobalFree, BOOL, HANDLE, HGLOBAL, HWND, POINT};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
        OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::System::Ole::{CF_HDROP, DROPEFFECT_COPY, DROPEFFECT_MOVE};
    use windows::Win32::UI::Shell::{
        DragQueryFileW, SHFileOperationW, DROPFILES, FOF_ALLOWUNDO, FOF_NOCONFIRMATION,
        FOF_NOERRORUI, FOF_SILENT, FOF_WANTNUKEWARNING, FO_DELETE, HDROP, SHFILEOPSTRUCTW,
    };

    use super::{ClipboardPayload, TransferMode};

    const CLIPBOARD_OPEN_ATTEMPTS: usize = 6;
    const CLIPBOARD_OPEN_DELAY: Duration = Duration::from_millis(15);

    struct ClipboardGuard;

    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseClipboard();
            }
        }
    }

    struct GlobalAllocation(Option<HGLOBAL>);

    impl GlobalAllocation {
        fn new(size: usize) -> Result<Self, String> {
            let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, size) }
                .map_err(|error| format!("could not allocate clipboard memory: {error}"))?;
            Ok(Self(Some(handle)))
        }

        fn handle(&self) -> HGLOBAL {
            self.0.expect("global allocation handle")
        }

        fn release(mut self) {
            self.0 = None;
        }
    }

    impl Drop for GlobalAllocation {
        fn drop(&mut self) {
            if let Some(handle) = self.0.take() {
                unsafe {
                    let _ = GlobalFree(handle);
                }
            }
        }
    }

    pub fn clipboard_has_files() -> bool {
        unsafe { IsClipboardFormatAvailable(CF_HDROP.0 as u32).is_ok() }
    }

    pub fn write_file_clipboard(paths: &[PathBuf], mode: TransferMode) -> Result<(), String> {
        if paths.is_empty() {
            return Err("no paths were selected".into());
        }

        let drop_bytes = encode_dropfiles(paths)?;
        let drop_allocation = write_global_bytes(&drop_bytes)?;
        let effect = match mode {
            TransferMode::Copy => DROPEFFECT_COPY.0,
            TransferMode::Move => DROPEFFECT_MOVE.0,
        };
        let effect_allocation = write_global_bytes(&effect.to_ne_bytes())?;
        let preferred_effect_format =
            unsafe { RegisterClipboardFormatW(w!("Preferred DropEffect")) };
        if preferred_effect_format == 0 {
            return Err("could not register Preferred DropEffect".into());
        }

        let _guard = open_clipboard()?;
        unsafe {
            EmptyClipboard().map_err(|error| format!("could not clear clipboard: {error}"))?;
            SetClipboardData(CF_HDROP.0 as u32, HANDLE(drop_allocation.handle().0))
                .map_err(|error| format!("could not publish file paths: {error}"))?;
        }
        drop_allocation.release();

        unsafe {
            SetClipboardData(
                preferred_effect_format,
                HANDLE(effect_allocation.handle().0),
            )
            .map_err(|error| format!("could not publish copy/cut mode: {error}"))?;
        }
        effect_allocation.release();
        Ok(())
    }

    pub fn read_file_clipboard() -> Result<Option<ClipboardPayload>, String> {
        if !clipboard_has_files() {
            return Ok(None);
        }

        let _guard = open_clipboard()?;
        let drop_handle = unsafe { GetClipboardData(CF_HDROP.0 as u32) }
            .map_err(|error| format!("could not read file clipboard: {error}"))?;
        let drop = HDROP(drop_handle.0);
        let count = unsafe { DragQueryFileW(drop, u32::MAX, None) };
        let mut paths = Vec::with_capacity(count as usize);
        for index in 0..count {
            let length = unsafe { DragQueryFileW(drop, index, None) };
            if length == 0 {
                continue;
            }
            let mut buffer = vec![0_u16; length as usize + 1];
            let written = unsafe { DragQueryFileW(drop, index, Some(&mut buffer)) };
            if written > 0 {
                paths.push(PathBuf::from(OsString::from_wide(
                    &buffer[..written as usize],
                )));
            }
        }

        if paths.is_empty() {
            return Ok(None);
        }
        Ok(Some(ClipboardPayload {
            paths,
            mode: read_preferred_drop_effect(),
        }))
    }

    pub fn recycle_paths(paths: &[PathBuf]) -> Result<(), String> {
        if paths.is_empty() {
            return Ok(());
        }
        let from = encode_shell_multistring(paths)?;
        let flags = FOF_ALLOWUNDO.0
            | FOF_NOCONFIRMATION.0
            | FOF_NOERRORUI.0
            | FOF_SILENT.0
            | FOF_WANTNUKEWARNING.0;
        let mut operation = SHFILEOPSTRUCTW {
            hwnd: HWND(ptr::null_mut()),
            wFunc: FO_DELETE,
            pFrom: PCWSTR(from.as_ptr()),
            pTo: PCWSTR::null(),
            fFlags: flags as u16,
            fAnyOperationsAborted: BOOL(0),
            hNameMappings: ptr::null_mut(),
            lpszProgressTitle: PCWSTR::null(),
        };

        let result = unsafe { SHFileOperationW(&mut operation) };
        if result != 0 {
            return Err(format!(
                "Windows Shell could not move the selection to Recycle Bin (code {result})"
            ));
        }
        if operation.fAnyOperationsAborted.as_bool() {
            return Err("Recycle Bin operation was cancelled by Windows".into());
        }
        Ok(())
    }

    fn open_clipboard() -> Result<ClipboardGuard, String> {
        let mut last_error = None;
        for _ in 0..CLIPBOARD_OPEN_ATTEMPTS {
            match unsafe { OpenClipboard(HWND(ptr::null_mut())) } {
                Ok(()) => return Ok(ClipboardGuard),
                Err(error) => {
                    last_error = Some(error);
                    thread::sleep(CLIPBOARD_OPEN_DELAY);
                }
            }
        }
        Err(format!(
            "clipboard is busy: {}",
            last_error
                .map(|error| error.to_string())
                .unwrap_or_else(|| "unknown Windows error".into())
        ))
    }

    fn write_global_bytes(bytes: &[u8]) -> Result<GlobalAllocation, String> {
        let allocation = GlobalAllocation::new(bytes.len())?;
        let pointer = unsafe { GlobalLock(allocation.handle()) };
        if pointer.is_null() {
            return Err("could not lock clipboard memory".into());
        }
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), pointer.cast::<u8>(), bytes.len());
            let _ = GlobalUnlock(allocation.handle());
        }
        Ok(allocation)
    }

    fn encode_dropfiles(paths: &[PathBuf]) -> Result<Vec<u8>, String> {
        let path_list = encode_shell_multistring(paths)?;
        let header = DROPFILES {
            pFiles: size_of::<DROPFILES>() as u32,
            pt: POINT { x: 0, y: 0 },
            fNC: BOOL(0),
            fWide: BOOL(1),
        };
        let mut bytes = vec![0_u8; size_of::<DROPFILES>() + path_list.len() * size_of::<u16>()];
        unsafe {
            ptr::write_unaligned(bytes.as_mut_ptr().cast::<DROPFILES>(), header);
            ptr::copy_nonoverlapping(
                path_list.as_ptr().cast::<u8>(),
                bytes.as_mut_ptr().add(size_of::<DROPFILES>()),
                path_list.len() * size_of::<u16>(),
            );
        }
        Ok(bytes)
    }

    fn encode_shell_multistring(paths: &[PathBuf]) -> Result<Vec<u16>, String> {
        let mut encoded = Vec::new();
        for path in paths {
            if path.as_os_str().is_empty() {
                return Err("clipboard path cannot be empty".into());
            }
            encoded.extend(path.as_os_str().encode_wide());
            encoded.push(0);
        }
        encoded.push(0);
        Ok(encoded)
    }

    fn read_preferred_drop_effect() -> TransferMode {
        let format = unsafe { RegisterClipboardFormatW(w!("Preferred DropEffect")) };
        if format == 0 || unsafe { IsClipboardFormatAvailable(format).is_err() } {
            return TransferMode::Copy;
        }
        let Ok(handle) = (unsafe { GetClipboardData(format) }) else {
            return TransferMode::Copy;
        };
        let global = HGLOBAL(handle.0);
        let pointer = unsafe { GlobalLock(global) };
        if pointer.is_null() {
            return TransferMode::Copy;
        }
        let effect = unsafe { pointer.cast::<u32>().read_unaligned() };
        unsafe {
            let _ = GlobalUnlock(global);
        }
        if effect & DROPEFFECT_MOVE.0 != 0 {
            TransferMode::Move
        } else {
            TransferMode::Copy
        }
    }

    #[cfg(test)]
    mod tests {
        use super::encode_shell_multistring;
        use std::path::PathBuf;

        #[test]
        fn shell_path_list_is_double_null_terminated() {
            let encoded = encode_shell_multistring(&[
                PathBuf::from(r"C:\Products\1001"),
                PathBuf::from(r"C:\Products\1002"),
            ])
            .expect("encode path list");

            assert!(encoded.len() > 2);
            assert_eq!(encoded[encoded.len() - 2..], [0, 0]);
            assert_eq!(encoded.iter().filter(|unit| **unit == 0).count(), 3);
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use std::path::PathBuf;

    use super::{ClipboardPayload, TransferMode};

    pub fn clipboard_has_files() -> bool {
        false
    }

    pub fn write_file_clipboard(_paths: &[PathBuf], _mode: TransferMode) -> Result<(), String> {
        Err("Windows Shell clipboard is only available on Windows".into())
    }

    pub fn read_file_clipboard() -> Result<Option<ClipboardPayload>, String> {
        Ok(None)
    }

    pub fn recycle_paths(_paths: &[PathBuf]) -> Result<(), String> {
        Err("Recycle Bin is only available on Windows".into())
    }
}

pub fn clipboard_has_files() -> bool {
    platform::clipboard_has_files()
}

pub fn write_file_clipboard(paths: &[PathBuf], mode: TransferMode) -> Result<(), String> {
    platform::write_file_clipboard(paths, mode)
}

pub fn read_file_clipboard() -> Result<Option<ClipboardPayload>, String> {
    platform::read_file_clipboard()
}

pub fn recycle_paths(paths: &[PathBuf]) -> Result<(), String> {
    platform::recycle_paths(paths)
}
