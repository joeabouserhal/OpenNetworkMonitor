//! Refuses to run elevated at sign-in from a place ordinary programs can modify.
//!
//! The startup task runs the app with the highest available rights. If the file, or
//! the folder holding it, can be changed by a non-administrator, anything running as
//! that user could replace the app (or plant a DLL beside it) and be started elevated
//! at the next sign-in without a prompt. Registration is therefore only allowed when
//! only SYSTEM, administrators and the installer can write to the app's location.
use std::{
    ffi::c_void,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
};
use windows_sys::Win32::{
    Foundation::{ERROR_SUCCESS, LocalFree},
    Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
        Authorization::{ConvertSidToStringSidW, GetNamedSecurityInfoW, SE_FILE_OBJECT},
        DACL_SECURITY_INFORMATION, GetAce, GetAclInformation, INHERIT_ONLY_ACE,
        OWNER_SECURITY_INFORMATION, PSID,
    },
};

/// ACE type of an allow entry (`ACCESS_ALLOWED_ACE_TYPE`).
const ALLOW_ENTRY: u32 = 0;
const FILE_WRITE_DATA: u32 = 0x2;
const FILE_APPEND_DATA: u32 = 0x4;
const FILE_DELETE_CHILD: u32 = 0x40;
const DELETE: u32 = 0x1_0000;
const WRITE_DAC: u32 = 0x4_0000;
const WRITE_OWNER: u32 = 0x8_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const GENERIC_ALL: u32 = 0x1000_0000;

/// Rights that let a principal change the app itself or what it loads from its folder.
const CHANGE_CONTENT: u32 = FILE_WRITE_DATA
    | FILE_APPEND_DATA
    | FILE_DELETE_CHILD
    | DELETE
    | WRITE_DAC
    | WRITE_OWNER
    | GENERIC_WRITE
    | GENERIC_ALL;
/// Rights that let a principal swap a whole folder out from under the app.
const REPLACE_FOLDER: u32 = FILE_DELETE_CHILD | DELETE | WRITE_DAC | WRITE_OWNER | GENERIC_ALL;

/// One allow entry of an access list, reduced to what the decision needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub sid: String,
    pub mask: u32,
    /// Only passed on to children, so it does not apply to the object itself.
    pub inherit_only: bool,
}

/// A file or folder's owner and allow entries; `grants` is `None` for a null access
/// list, which gives everyone full control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Access {
    pub owner: String,
    pub grants: Option<Vec<Grant>>,
}

/// SYSTEM, the Administrators group and service SIDs such as TrustedInstaller.
fn trusted(sid: &str) -> bool {
    sid == "S-1-5-18" || sid == "S-1-5-32-544" || sid.starts_with("S-1-5-80-")
}

/// Who, besides trusted principals, holds one of the `rights` on this object.
pub fn untrusted_writers(access: &Access, rights: u32) -> Vec<String> {
    let Some(grants) = &access.grants else {
        return vec!["Everyone".to_owned()];
    };
    let mut found = Vec::new();
    // The owner can always rewrite the access list, whatever it currently says.
    if !trusted(&access.owner) {
        found.push(access.owner.clone());
    }
    for grant in grants {
        if !grant.inherit_only
            && grant.mask & rights != 0
            && !trusted(&grant.sid)
            && !found.contains(&grant.sid)
        {
            found.push(grant.sid.clone());
        }
    }
    found
}

fn describe(sid: &str) -> &str {
    match sid {
        "S-1-1-0" => "Everyone",
        "S-1-5-11" => "Authenticated Users",
        "S-1-5-32-545" => "Users",
        _ => "a non-administrator account",
    }
}

/// Why `exe` must not run elevated at sign-in, or `None` when only trusted principals
/// can change it. `access` reads the owner and allow entries of one path.
pub fn risk_with(exe: &Path, access: impl Fn(&Path) -> Option<Access>) -> Option<String> {
    let parent = exe.parent()?;
    let mut checks: Vec<(PathBuf, u32, &str)> = vec![
        (exe.to_path_buf(), CHANGE_CONTENT, "The app file"),
        (parent.to_path_buf(), CHANGE_CONTENT, "The app's folder"),
    ];
    checks.extend(parent.ancestors().skip(1).map(|folder| {
        (
            folder.to_path_buf(),
            REPLACE_FOLDER,
            "A folder above the app",
        )
    }));
    for (path, rights, what) in checks {
        let Some(access) = access(&path) else {
            return Some(format!(
                "Its permissions could not be read.\n{}",
                path.display()
            ));
        };
        let writers = untrusted_writers(&access, rights);
        if !writers.is_empty() {
            let names: Vec<&str> = writers.iter().map(|sid| describe(sid)).collect();
            return Some(format!(
                "{what} can be changed by {}.\n{}",
                names.join(", "),
                path.display()
            ));
        }
    }
    None
}

/// [`risk_with`] against the real file system permissions.
pub fn risk(exe: &Path) -> Option<String> {
    risk_with(exe, read_access)
}

fn sid_string(sid: PSID) -> Option<String> {
    let mut text = std::ptr::null_mut();
    // SAFETY: `sid` points into a live security descriptor; the returned string is
    // null-terminated, copied out and freed with LocalFree.
    unsafe {
        if ConvertSidToStringSidW(sid, &mut text) == 0 {
            return None;
        }
        let length = (0..256).find(|&i| *text.add(i) == 0)?;
        let result = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
        LocalFree(text.cast());
        Some(result)
    }
}

fn read_access(path: &Path) -> Option<Access> {
    let text = path.as_os_str().to_string_lossy();
    let plain = text.trim_start_matches(r"\\?\");
    let wide: Vec<u16> = std::ffi::OsStr::new(plain)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut owner: PSID = std::ptr::null_mut();
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut descriptor: *mut c_void = std::ptr::null_mut();
    // SAFETY: `wide` is null-terminated and outlives the call. The out pointers are
    // valid, and everything they return points into `descriptor`, which is freed once
    // below after the data is copied out.
    let status = unsafe {
        GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            &mut dacl,
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let result = (|| {
        let owner = sid_string(owner)?;
        if dacl.is_null() {
            return Some(Access {
                owner,
                grants: None,
            });
        }
        // SAFETY: `dacl` is a valid access list inside `descriptor`; ACE pointers
        // returned by GetAce stay inside it and are read-only here.
        unsafe {
            let mut info: ACL_SIZE_INFORMATION = std::mem::zeroed();
            if GetAclInformation(
                dacl,
                (&mut info as *mut ACL_SIZE_INFORMATION).cast(),
                std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            ) == 0
            {
                return None;
            }
            let mut grants = Vec::new();
            for index in 0..info.AceCount {
                let mut ace: *mut c_void = std::ptr::null_mut();
                if GetAce(dacl, index, &mut ace) == 0 {
                    return None;
                }
                let header = &*ace.cast::<ACE_HEADER>();
                if u32::from(header.AceType) != ALLOW_ENTRY {
                    continue;
                }
                let allowed = &*ace.cast::<ACCESS_ALLOWED_ACE>();
                let sid = sid_string((&allowed.SidStart as *const u32).cast_mut().cast())?;
                grants.push(Grant {
                    sid,
                    mask: allowed.Mask,
                    inherit_only: u32::from(header.AceFlags) & INHERIT_ONLY_ACE != 0,
                });
            }
            Some(Access {
                owner,
                grants: Some(grants),
            })
        }
    })();
    // SAFETY: `descriptor` was allocated by GetNamedSecurityInfoW for this call.
    unsafe { LocalFree(descriptor) };
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYSTEM: &str = "S-1-5-18";
    const ADMINS: &str = "S-1-5-32-544";
    const USERS: &str = "S-1-5-32-545";
    const INSTALLER: &str = "S-1-5-80-956008885-3418522649";
    const ME: &str = "S-1-5-21-1-2-3-1001";

    fn grant(sid: &str, mask: u32) -> Grant {
        Grant {
            sid: sid.into(),
            mask,
            inherit_only: false,
        }
    }
    fn locked_down() -> Access {
        Access {
            owner: INSTALLER.into(),
            grants: Some(vec![
                grant(INSTALLER, GENERIC_ALL),
                grant(ADMINS, 0x1F_01FF),
                grant(SYSTEM, 0x1F_01FF),
                grant(USERS, 0x1200A9),
            ]),
        }
    }

    #[test]
    fn only_trusted_principals_may_hold_write_rights() {
        assert!(untrusted_writers(&locked_down(), CHANGE_CONTENT).is_empty());

        let mut writable = locked_down();
        writable.grants.as_mut().unwrap().push(grant(USERS, 0x2));
        assert_eq!(untrusted_writers(&writable, CHANGE_CONTENT), [USERS]);
        assert!(
            untrusted_writers(&writable, REPLACE_FOLDER).is_empty(),
            "Adding files does not let a folder be swapped out"
        );

        let mut modify = locked_down();
        modify.grants.as_mut().unwrap().push(grant(USERS, 0x1301BF));
        assert_eq!(untrusted_writers(&modify, REPLACE_FOLDER), [USERS]);
    }

    #[test]
    fn owners_null_lists_and_inherit_only_entries_are_handled() {
        let mut owned = locked_down();
        owned.owner = ME.into();
        assert_eq!(untrusted_writers(&owned, CHANGE_CONTENT), [ME]);

        let open = Access {
            owner: ADMINS.into(),
            grants: None,
        };
        assert_eq!(untrusted_writers(&open, CHANGE_CONTENT), ["Everyone"]);

        let mut inherit = locked_down();
        inherit.grants.as_mut().unwrap().push(Grant {
            inherit_only: true,
            ..grant(USERS, 0x1301BF)
        });
        assert!(
            untrusted_writers(&inherit, CHANGE_CONTENT).is_empty(),
            "Rights meant only for children do not apply to the folder itself"
        );
    }

    #[test]
    fn the_file_its_folder_and_the_folders_above_are_all_checked() {
        let exe = Path::new(r"C:\Apps\Tools\Monitor.exe");
        let safe = |_: &Path| Some(locked_down());
        assert_eq!(risk_with(exe, safe), None);

        let unsafe_at = |bad: &'static str| {
            move |path: &Path| {
                let mut access = locked_down();
                if path == Path::new(bad) {
                    access.grants.as_mut().unwrap().push(grant(USERS, 0x1301BF));
                }
                Some(access)
            }
        };
        for bad in [r"C:\Apps\Tools\Monitor.exe", r"C:\Apps\Tools", r"C:\Apps"] {
            let reason = risk_with(exe, unsafe_at(bad)).expect(bad);
            assert!(reason.contains(bad) && reason.contains("Users"), "{reason}");
        }
        assert!(
            risk_with(exe, |_| None)
                .unwrap()
                .contains("could not be read"),
            "Unreadable permissions are treated as unsafe"
        );
    }

    #[test]
    fn system_folders_pass_and_user_writable_folders_fail_on_this_machine() {
        let system = Path::new(r"C:\Windows\System32\notepad.exe");
        if system.exists() {
            assert_eq!(risk(system), None);
        }
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("Monitor.exe");
        std::fs::write(&exe, b"x").unwrap();
        assert!(
            risk(&exe).is_some(),
            "A file in the user's own temp folder can be changed without administrator rights"
        );
    }
}
