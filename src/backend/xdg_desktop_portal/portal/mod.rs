use std::{
    ffi::{CStr, CString, OsStr},
    os::unix::ffi::OsStrExt,
    path::PathBuf,
};

mod ffi;

mod file_dialog;

mod libdbus;
use libdbus::*;

pub use file_dialog::{FileFilter, FilePath, HandleToken, OpenFileOptions, SaveFileOptions};

pub fn uris_to_paths(uris: Vec<CString>) -> Vec<PathBuf> {
    uris.into_iter()
        .filter_map(|uri| {
            let bytes: Vec<u8> = percent_encoding::percent_decode(uri.as_bytes()).collect();
            let Some(path) = bytes.strip_prefix(b"file://") else {
                log::error!("Ignoring uri: {bytes:?} lacks `file://`");
                return None;
            };
            Some(PathBuf::from(OsStr::from_bytes(path)))
        })
        .collect()
}

/// What the portal answered: no `uris` when the user cancelled.
#[derive(Debug, Default, PartialEq)]
pub struct Response {
    pub uris: Vec<CString>,
    /// The label of the filter selected when the dialog closed. Portals may
    /// leave it out.
    pub current_filter: Option<CString>,
}

pub fn open_file(opts: OpenFileOptions) -> Option<Vec<CString>> {
    let mut conn = Connection::new()?;

    let handle_path = generate_response_path(&mut conn, &opts.handle_token);
    register_response_listener(&mut conn, &handle_path);

    let reply = conn.send_and_block(&Message::open_file(opts));

    if conn.err().is_err() {
        log::error!("OpenFile failed: {}", conn.err());
        return None;
    }

    let reply = reply?;

    let mut iter = MessageIter::from_msg(&reply);

    let Some(got_handle_path) = iter.get_object_path() else {
        log::error!("Response object path is missing");
        return None;
    };

    if handle_path != got_handle_path {
        log::debug!("Detected ancient version of xdg portal, attempting fallback");
        register_response_listener(&mut conn, &got_handle_path);
    }

    wait_for_response(&mut conn, &handle_path).map(|response| response.uris)
}

pub fn save_file(opts: SaveFileOptions) -> Option<Response> {
    let mut conn = Connection::new()?;

    let handle_path = generate_response_path(&mut conn, &opts.handle_token);
    register_response_listener(&mut conn, &handle_path);

    let reply = conn.send_and_block(&Message::save_file(opts));

    if conn.err().is_err() {
        log::error!("OpenFile failed: {}", conn.err());
        return None;
    }

    let reply = reply?;

    let mut iter = MessageIter::from_msg(&reply);

    let Some(got_handle_path) = iter.get_object_path() else {
        log::error!("Response object path is missing");
        return None;
    };

    if handle_path != got_handle_path {
        log::debug!("Detected ancient version of xdg portal, attempting fallback");
        register_response_listener(&mut conn, &got_handle_path);
    }

    wait_for_response(&mut conn, &handle_path)
}

fn generate_response_path(conn: &mut Connection, handle_token: &HandleToken) -> CString {
    let unique_name = conn.get_unique_name();
    let unique_name = unique_name.to_str().unwrap();
    let unique_identifier = unique_name.trim_start_matches(':').replace('.', "_");
    let handle_token = handle_token.0.to_str().unwrap();
    CString::new(format!(
        "/org/freedesktop/portal/desktop/request/{unique_identifier}/{handle_token}"
    ))
    .unwrap()
}

fn register_response_listener(conn: &mut Connection, handle_path: &CStr) {
    conn.add_match(
        &CString::new(
            [
                "type='signal'",
                "sender='org.freedesktop.portal.Desktop'",
                &format!("path='{}'", handle_path.to_str().unwrap()),
                "interface='org.freedesktop.portal.Request'",
                "member='Response'",
            ]
            .join(","),
        )
        .unwrap(),
    );

    if conn.err().is_err() {
        log::error!("Failed to add match rule: {}", conn.err());
    }

    conn.flush();
}

#[derive(Debug)]
enum ResponseCode {
    Success = 0,
    // Cancelled = 1,
    // Other = 2,
}

fn wait_for_response(conn: &mut Connection, handle_path: &CStr) -> Option<Response> {
    loop {
        conn.read_write(-1);
        while let Some(signal) = conn.pop_message() {
            if signal.is_signal(c"org.freedesktop.portal.Request", c"Response") {
                let Some(path) = signal.get_path() else {
                    log::error!("Response signal is missing a path");
                    return None;
                };

                if path == handle_path {
                    return parse_response(&signal);
                }
            }
        }
    }
}

fn parse_response(msg: &Message) -> Option<Response> {
    let mut iter = MessageIter::from_msg(msg);

    let Some(response_code) = iter.get_u32() else {
        log::error!("Response code missing");
        return None;
    };
    if response_code != ResponseCode::Success as u32 {
        return Some(Response::default());
    }

    if !iter.next() {
        log::error!("Body of the response is empty");
        return None;
    }

    if iter.get_arg_type() != ffi::DBUS_TYPE_ARRAY {
        log::error!("Body of the response is not an array");
        return None;
    }

    let mut dict_iter = iter.iter_recurse();
    let mut uris = None;
    let mut current_filter = None;

    while dict_iter.get_arg_type() == ffi::DBUS_TYPE_DICT_ENTRY {
        let mut entry_iter = dict_iter.iter_recurse();

        let Some(key) = entry_iter.get_string() else {
            log::error!("Wrong type of a dict key");
            continue;
        };

        entry_iter.next();
        if key.as_c_str() == c"uris" {
            if entry_iter.get_arg_type() == ffi::DBUS_TYPE_VARIANT {
                let mut var_iter = entry_iter.iter_recurse();
                uris = Some(var_iter.get_string_array());
            } else {
                log::error!(
                    "Response.uris type {} != VARIANT",
                    entry_iter.get_arg_type()
                );
            }
        } else if key.as_c_str() == c"current_filter" {
            current_filter = filter_label(&mut entry_iter);
        }

        dict_iter.next();
    }

    let Some(uris) = uris else {
        log::error!("Response.uris was not found");
        return None;
    };
    Some(Response {
        uris,
        current_filter,
    })
}

/// The label of a `current_filter` variant, whose filter is a `(sa(us))`
/// struct of the label and its patterns.
fn filter_label(entry_iter: &mut MessageIter) -> Option<CString> {
    if entry_iter.get_arg_type() != ffi::DBUS_TYPE_VARIANT {
        log::error!(
            "Response.current_filter type {} != VARIANT",
            entry_iter.get_arg_type()
        );
        return None;
    }
    let mut filter_iter = entry_iter.iter_recurse();
    if filter_iter.get_arg_type() != ffi::DBUS_TYPE_STRUCT {
        log::error!(
            "Response.current_filter type {} != STRUCT",
            filter_iter.get_arg_type()
        );
        return None;
    }
    filter_iter.iter_recurse().get_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `Response` signal body: the response code, then `results`, holding
    /// `uris` and, when given, `current_filter`.
    fn response_message(code: u32, uris: &[&CStr], current_filter: Option<&CStr>) -> Message {
        assert!(
            ffi::Libdbus::open_libdbus().is_some(),
            "the portal tests need libdbus-1"
        );
        let mut msg = Message::new_method_call(
            c"org.freedesktop.portal.Desktop",
            c"/org/freedesktop/portal/desktop",
            c"org.freedesktop.portal.Request",
            c"Response",
        )
        .expect("a D-Bus message");
        let mut iter = MessageIter::init_append(&mut msg);
        iter.append_u32(code);
        iter.with_container(ffi::DBUS_TYPE_ARRAY, Some(c"{sv}"), |dict| {
            dict.with_dict_entry(c"uris", c"as", |variant| {
                variant.with_container(ffi::DBUS_TYPE_ARRAY, Some(c"s"), |array| {
                    for uri in uris {
                        array.append_string(uri);
                    }
                });
            });
            if let Some(label) = current_filter {
                dict.with_dict_entry(c"current_filter", c"(sa(us))", |variant| {
                    variant.with_container(ffi::DBUS_TYPE_STRUCT, None, |filter| {
                        filter.append_string(label);
                        filter.with_container(ffi::DBUS_TYPE_ARRAY, Some(c"(us)"), |globs| {
                            globs.with_container(ffi::DBUS_TYPE_STRUCT, None, |glob| {
                                glob.append_u32(0);
                                glob.append_string(c"*.dxf");
                            });
                        });
                    });
                });
            }
        });
        msg
    }

    #[test]
    fn a_response_reports_the_current_filter() {
        let msg = response_message(0, &[c"file:///plan.dxf"], Some(c"DXF (.dxf)"));
        assert_eq!(
            parse_response(&msg),
            Some(Response {
                uris: vec![c"file:///plan.dxf".to_owned()],
                current_filter: Some(c"DXF (.dxf)".to_owned()),
            })
        );
    }

    #[test]
    fn a_response_without_current_filter_reports_none() {
        let msg = response_message(0, &[c"file:///plan.dxf"], None);
        assert_eq!(
            parse_response(&msg),
            Some(Response {
                uris: vec![c"file:///plan.dxf".to_owned()],
                current_filter: None,
            })
        );
    }

    #[test]
    fn a_cancelled_response_has_no_uris() {
        let msg = response_message(1, &[], None);
        assert_eq!(parse_response(&msg), Some(Response::default()));
    }
}
