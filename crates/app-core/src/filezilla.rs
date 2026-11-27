//! Import of FileZilla Site Manager entries (`sitemanager.xml`, or an
//! export from *File → Export…*). FTP, explicit FTPS and SFTP sites become
//! sessions under a "FileZilla" folder, keeping FileZilla's folders.
//! Passwords stored in the file are never imported.

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::AppError;
use crate::sessions::{AuthMethod, SessionInput, SessionProtocol};
use crate::{ImportCandidate, ImportReport, SessionStore, import_candidates};

/// Folder the imported sites go to (FileZilla's folders below it).
pub const FILEZILLA_FOLDER: &str = "FileZilla";

/// Whether `text` is a FileZilla Site Manager file.
pub fn is_filezilla(text: &str) -> bool {
    text.contains("<FileZilla3") && text.contains("<Servers")
}

#[derive(Default, Debug)]
struct Site {
    name: String,
    host: String,
    port: Option<u16>,
    protocol: Option<u32>,
    user: String,
    logon_type: Option<u32>,
    key_file: String,
    has_password: bool,
    folder: Vec<String>,
}

/// `&amp;`, `&lt;`, `&#38;`… as text.
fn entity(name: &str) -> String {
    match name {
        "amp" => "&".into(),
        "lt" => "<".into(),
        "gt" => ">".into(),
        "quot" => "\"".into(),
        "apos" => "'".into(),
        n => n
            .strip_prefix("#x")
            .and_then(|h| u32::from_str_radix(h, 16).ok())
            .or_else(|| n.strip_prefix('#').and_then(|d| d.parse().ok()))
            .and_then(char::from_u32)
            .map(String::from)
            .unwrap_or_default(),
    }
}

/// Reads the sites with their folder paths; `None` when the XML is broken.
fn read_sites(text: &str) -> Option<Vec<Site>> {
    let mut reader = Reader::from_str(text);
    let mut sites = Vec::new();
    // Element names from the root, and the text of the current element.
    let mut path: Vec<String> = Vec::new();
    let mut folders: Vec<String> = Vec::new();
    // A folder's name is its first text, before any child element.
    let mut folder_named: Vec<bool> = Vec::new();
    let mut current: Option<Site> = None;
    let mut text_buf = String::new();
    loop {
        match reader.read_event().ok()? {
            Event::Start(e) => {
                let name = e.local_name().as_ref().to_owned();
                if let Some(parent) = path.last()
                    && parent == "Folder"
                    && let Some(named) = folder_named.last_mut()
                    && !*named
                {
                    folders.push(text_buf.trim().to_owned());
                    *named = true;
                }
                match name.as_str() {
                    "Folder" => folder_named.push(false),
                    "Server" => {
                        current = Some(Site {
                            folder: folders.clone(),
                            ..Site::default()
                        })
                    }
                    _ => {}
                }
                path.push(name);
                text_buf.clear();
            }
            Event::Empty(e) => {
                let name = e.local_name().as_ref().to_owned();
                if name == "Server" {
                    continue;
                }
                text_buf.clear();
            }
            Event::Text(t) => text_buf.push_str(&t.xml10_content()),
            Event::CData(t) => text_buf.push_str(&t),
            Event::GeneralRef(r) => text_buf.push_str(&entity(&r.into_inner())),
            Event::End(_) => {
                let name = path.pop()?;
                let value = text_buf.trim().to_owned();
                match name.as_str() {
                    "Folder" => {
                        if folder_named.pop() == Some(true) {
                            folders.pop();
                        }
                    }
                    "Server" => sites.extend(current.take()),
                    field => {
                        if let Some(s) = current.as_mut()
                            && path.last().map(String::as_str) == Some("Server")
                        {
                            match field {
                                "Name" => s.name = value,
                                "Host" => s.host = value,
                                "Port" => s.port = value.parse().ok(),
                                "Protocol" => s.protocol = value.parse().ok(),
                                "User" => s.user = value,
                                "Logontype" => s.logon_type = value.parse().ok(),
                                "Keyfile" => s.key_file = value,
                                "Pass" => s.has_password = !value.is_empty(),
                                _ => {}
                            }
                        }
                    }
                }
                text_buf.clear();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Some(sites)
}

/// Parses the sites; returns the candidates and notes about entries that
/// cannot be imported.
pub fn parse_filezilla(text: &str) -> Result<(Vec<ImportCandidate>, Vec<String>), AppError> {
    let sites = read_sites(text)
        .ok_or_else(|| AppError::invalid("import", "the FileZilla file is not valid XML"))?;
    let mut out: Vec<ImportCandidate> = Vec::new();
    let mut notes = Vec::new();
    let mut passwords_left_out = false;
    for s in sites {
        let name = if s.name.trim().is_empty() {
            s.host.clone()
        } else {
            s.name.trim().to_owned()
        };
        // FileZilla protocol numbers: 0 FTP (TLS if available), 1 SFTP,
        // 3 implicit FTPS, 4 explicit FTPS, 6 plain FTP.
        let (protocol, files_only) = match s.protocol.unwrap_or(0) {
            0 | 4 => (SessionProtocol::Ftps, false),
            6 => (SessionProtocol::Ftp, false),
            1 => (SessionProtocol::Ssh, true),
            3 => {
                notes.push(format!("{name}: implicit FTPS (port 990) is not supported"));
                continue;
            }
            other => {
                notes.push(format!(
                    "{name}: FileZilla protocol {other} is not supported"
                ));
                continue;
            }
        };
        if s.protocol.unwrap_or(0) == 0 {
            notes.push(format!(
                "{name}: imported as FTPS (FileZilla tried TLS first); change the type to FTP if the server has no TLS"
            ));
        }
        passwords_left_out |= s.has_password;
        // Logon type 0 is anonymous.
        let user = if s.logon_type == Some(0) {
            String::new()
        } else {
            s.user.trim().to_owned()
        };
        let mut auth = AuthMethod::Password;
        if files_only && s.logon_type == Some(5) && !s.key_file.trim().is_empty() {
            let path = s.key_file.trim();
            if path.to_ascii_lowercase().ends_with(".ppk")
                || !std::path::Path::new(path).is_absolute()
            {
                notes.push(format!(
                    "{name}: choose the key file again in the session ({path})"
                ));
            } else {
                auth = AuthMethod::KeyFile { path: path.into() };
            }
        }
        let default_port = if files_only { 22 } else { 21 };
        let mut folder = vec![FILEZILLA_FOLDER.to_owned()];
        folder.extend(
            s.folder
                .iter()
                .filter(|f| !f.is_empty())
                .map(|f| f.replace('/', "-")),
        );
        let input = SessionInput {
            name: name.clone(),
            host: s.host.trim().to_owned(),
            port: s.port.filter(|p| *p > 0).unwrap_or(default_port),
            username: user,
            auth,
            folder: Some(folder.join("/")),
            jump_via: None,
            protocol,
            domain: None,
            tags: Vec::new(),
            forward_agent: false,
            x11_forward: false,
            startup_command: None,
            files_only,
            mosh: false,
            appearance: None,
            icon: None,
            s3: None,
            legacy_ssh: false,
            telnet_login: false,
        };
        if let Err(e) = input.validate() {
            notes.push(format!("{name}: {e}"));
            continue;
        }
        if out.iter().any(|c| c.alias.eq_ignore_ascii_case(&name)) {
            notes.push(format!("{name}: duplicate name"));
            continue;
        }
        out.push(ImportCandidate {
            alias: name,
            input,
            warnings: Vec::new(),
            jump_alias: None,
        });
    }
    if passwords_left_out {
        notes.push(
            "Passwords in the file were not imported; you are asked at the first login.".to_owned(),
        );
    }
    Ok((out, notes))
}

/// Imports the FileZilla sites into the "FileZilla" folder. Existing names
/// are skipped.
pub fn import_filezilla(store: &SessionStore, text: &str) -> Result<ImportReport, AppError> {
    if text.len() > crate::settings::MAX_IMPORT_BYTES {
        return Err(AppError::invalid("import", "file is too large"));
    }
    let (candidates, notes) = parse_filezilla(text)?;
    let mut report = import_candidates(store, &candidates, &[])?;
    report.not_imported = notes;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<FileZilla3 version="3.66.0" platform="windows">
  <Servers>
    <Server>
      <Host>ftp.example.com</Host>
      <Port>21</Port>
      <Protocol>4</Protocol>
      <Type>0</Type>
      <User>bob</User>
      <Pass encoding="base64">c2VjcmV0</Pass>
      <Logontype>1</Logontype>
      <Name>Web &amp; mail</Name>
    </Server>
    <Folder expanded="1">Clients
      <Folder>Acme
        <Server>
          <Host>sftp.acme.test</Host>
          <Port>2222</Port>
          <Protocol>1</Protocol>
          <User>deploy</User>
          <Logontype>5</Logontype>
          <Keyfile>C:\keys\acme.ppk</Keyfile>
          <Name>acme files</Name>
        </Server>
      </Folder>
      <Server>
        <Host>mirror.example.org</Host>
        <Port>21</Port>
        <Protocol>0</Protocol>
        <User>anonymous</User>
        <Logontype>0</Logontype>
        <Name>mirror</Name>
      </Server>
      <Server>
        <Host>old.example.org</Host>
        <Port>990</Port>
        <Protocol>3</Protocol>
        <Name>old implicit</Name>
      </Server>
    </Folder>
  </Servers>
</FileZilla3>
"#;

    #[test]
    fn reads_sites_with_folders_and_protocols() {
        assert!(is_filezilla(SAMPLE));
        assert!(!is_filezilla("[Bookmarks]"));
        let (sites, notes) = parse_filezilla(SAMPLE).unwrap();
        let names: Vec<&str> = sites.iter().map(|c| c.alias.as_str()).collect();
        assert_eq!(names, ["Web & mail", "acme files", "mirror"]);

        let web = &sites[0].input;
        assert_eq!(
            (web.protocol, web.port, web.username.as_str()),
            (SessionProtocol::Ftps, 21, "bob")
        );
        assert_eq!(web.folder.as_deref(), Some("FileZilla"));

        let acme = &sites[1].input;
        assert!(acme.files_only && acme.protocol == SessionProtocol::Ssh);
        assert_eq!(acme.port, 2222);
        assert_eq!(acme.folder.as_deref(), Some("FileZilla/Clients/Acme"));
        assert_eq!(
            acme.auth,
            AuthMethod::Password,
            ".ppk keys must be chosen again"
        );

        let mirror = &sites[2].input;
        assert_eq!(mirror.username, "", "anonymous logon");
        assert_eq!(mirror.folder.as_deref(), Some("FileZilla/Clients"));

        assert!(
            notes.iter().any(|n| n.starts_with("old implicit:")),
            "{notes:?}"
        );
        assert!(
            notes
                .iter()
                .any(|n| n.starts_with("mirror:") && n.contains("FTPS"))
        );
        assert!(
            notes
                .iter()
                .any(|n| n.starts_with("Passwords in the file were not imported"))
        );
    }

    #[test]
    fn imports_into_the_store() {
        let store = SessionStore::open_in_memory().unwrap();
        let report = import_filezilla(&store, SAMPLE).unwrap();
        assert_eq!(report.created.len(), 3);
        assert!(
            store.list().unwrap().iter().all(|s| !s.has_saved_secret),
            "no passwords imported"
        );
        assert_eq!(import_filezilla(&store, SAMPLE).unwrap().skipped.len(), 3);
    }

    #[test]
    fn broken_xml_is_an_error() {
        assert!(parse_filezilla("<FileZilla3><Servers><Server></Servers>").is_err());
    }
}
