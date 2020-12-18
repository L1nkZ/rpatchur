use anyhow::Result;

/// Starts an executable file in a cross-platform way.
///
/// This is the Windows version.
#[cfg(windows)]
pub fn start_executable<I, S>(exe_path: &String, exe_arguments: I) -> Result<bool>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    // Fold parameter list into a String
    let exe_parameter = exe_arguments
        .into_iter()
        .fold(String::new(), |a: String, b| a + " \"" + b.as_ref() + "\"");
    windows::spawn_elevated_win32_process(exe_path, &exe_parameter)
}

/// Starts an executable file in a cross-platform way.
///
/// This is the non-Windows version.
#[cfg(not(windows))]
pub fn start_executable<I, S>(exe_path: &String, exe_arguments: I) -> Result<bool>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    use std::process::Command;

    let exe_arguments: Vec<String> = exe_arguments
        .into_iter()
        .map(|e| e.as_ref().into())
        .collect();
    Command::new(exe_path)
        .args(exe_arguments)
        .spawn()
        .map(|_| Ok(true))?
}

// Note: Taken from the rustup project
#[cfg(windows)]
mod windows {
    use anyhow::{anyhow, Result};
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    fn to_u16s<S: AsRef<OsStr>>(s: S) -> Result<Vec<u16>> {
        fn inner(s: &OsStr) -> Result<Vec<u16>> {
            let mut maybe_result: Vec<u16> = s.encode_wide().collect();
            if maybe_result.iter().any(|&u| u == 0) {
                return Err(anyhow!("strings passed to WinAPI cannot contain NULs"));
            }
            maybe_result.push(0);
            Ok(maybe_result)
        }
        inner(s.as_ref())
    }

    /// This function is required to start processes that require elevation, from
    /// a non-elevated process.
    pub fn spawn_elevated_win32_process<S>(path: S, parameter: S) -> Result<bool>
    where
        S: AsRef<OsStr>,
    {
        use std::ptr;
        use winapi::ctypes::c_int;
        use winapi::shared::minwindef::HINSTANCE;
        use winapi::shared::ntdef::LPCWSTR;
        use winapi::shared::windef::HWND;
        extern "system" {
            pub fn ShellExecuteW(
                hwnd: HWND,
                lpOperation: LPCWSTR,
                lpFile: LPCWSTR,
                lpParameters: LPCWSTR,
                lpDirectory: LPCWSTR,
                nShowCmd: c_int,
            ) -> HINSTANCE;
        }
        const SW_SHOW: c_int = 5;

        let path = to_u16s(path)?;
        let parameter = to_u16s(parameter)?;
        let operation = to_u16s("runas")?;
        let result = unsafe {
            ShellExecuteW(
                ptr::null_mut(),
                operation.as_ptr(),
                path.as_ptr(),
                parameter.as_ptr(),
                ptr::null(),
                SW_SHOW,
            )
        };
        Ok(result as usize > 32)
    }
}
