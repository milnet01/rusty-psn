use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use super::{PackageInfo, UpdateInfo};

#[derive(Debug)]
pub enum ParseError {
    ErrorCode(String),
    XmlParsing(quick_xml::Error),
    XmlEncodingError(quick_xml::encoding::EncodingError),
}

pub fn parse_response(response: &str, info: &mut UpdateInfo) -> Result<(), ParseError> {
    let mut reader = Reader::from_str(response);
    reader.config_mut().trim_text(true);

    let mut depth = 0;
    let mut title_element = false;
    let mut event_buf = Vec::new();

    let mut err_encountered = false;
    let mut err_code_encountered = false;

    loop {
        match reader.read_event_into(&mut event_buf) {
            Ok(Event::Start(e)) => {
                depth += 1;

                match e.name().as_ref() {
                    b"titlepatch" => {
                        if let Some(value) = attribute_value(&e, b"titleid") {
                            info.title_id = value;
                        }
                    }
                    b"tag" => {
                        if let Some(value) = attribute_value(&e, b"name") {
                            info.tag_name = value;
                        }
                    }
                    b"package" => parse_package(&e, info)?,
                    b"Error" => {
                        err_encountered = true;
                    }
                    b"Code" => {
                        if !err_encountered {
                            warn!("Code tag encountered without a preceeding Error tag, skipping it");
                            continue;
                        }

                        err_code_encountered = true;
                    }
                    name => {
                        if name.len() >= 5 && name[..5].eq_ignore_ascii_case(b"title") {
                            title_element = true;
                        }
                    }
                }
            }
            Ok(Event::End(_)) => {
                depth -= 1;
            }
            Ok(Event::Empty(e)) => {
                if e.name().as_ref() == b"package" {
                    parse_package(&e, info)?;
                }
            }
            Ok(Event::Text(e)) => {
                if title_element {
                    let title = e.decode().map_err(ParseError::XmlEncodingError)?;

                    title_element = false;
                    info.titles.push(title.to_string());
                } else if err_code_encountered {
                    let err_code_text = e.decode().map_err(ParseError::XmlEncodingError)?;
                    return Err(ParseError::ErrorCode(err_code_text.into()));
                }
            }
            Ok(Event::Eof) => break,
            _ => {}
        }
    }

    if err_encountered {
        warn!("Error tag encountered without a following Code tag");
    }

    if depth != 0 {
        warn!("Finished parsing xml with non-zero depth {depth}");
    }

    Ok(())
}

fn attribute_value(element: &BytesStart, key: &[u8]) -> Option<String> {
    element
        .attributes()
        .filter_map(|a| a.ok())
        .find(|a| a.key.as_ref() == key)
        .and_then(|a| a.unescape_value().ok())
        .map(|value| value.into_owned())
}

/// Parses a `<package>` element's attributes into a new entry of `info.packages`.
fn parse_package(element: &BytesStart, info: &mut UpdateInfo) -> Result<(), ParseError> {
    for attribute in element.attributes().filter_map(|a| a.ok()) {
        let key = attribute.key.as_ref();
        if key == b"version" {
            let value = attribute.unescape_value().map_err(ParseError::XmlParsing)?;
            info.packages.push(PackageInfo {
                version: value.into_owned(),
                ..PackageInfo::empty()
            });
            continue;
        }

        // Every other attribute applies to the package created by the preceding `version` attribute.
        let Some(package) = info.packages.last_mut() else {
            continue;
        };

        match key {
            b"size" => {
                let value = attribute.unescape_value().map_err(ParseError::XmlParsing)?;
                package.size = value.parse::<u64>().unwrap_or_default();
            }
            b"sha1sum" => package.sha1sum = attribute.unescape_value().map_err(ParseError::XmlParsing)?.into_owned(),
            b"url" => package.url = attribute.unescape_value().map_err(ParseError::XmlParsing)?.into_owned(),
            b"manifest_url" => package.manifest_url = attribute.unescape_value().map_err(ParseError::XmlParsing)?.into_owned(),
            _ => {}
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::psn::utils::PlaformVariant;

    #[test]
    fn parses_packages_and_title() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<titlepatch titleid="NPUB30826">
  <tag name="NPUB30826_T2" popup="true">
    <package version="01.01" size="100" sha1sum="ABC" url="http://a/x.pkg">
      <paramsfo><TITLE>Some Game</TITLE></paramsfo>
    </package>
    <package version="01.02" size="200" sha1sum="DEF" url="http://a/y.pkg" manifest_url="http://a/m.json"/>
  </tag>
</titlepatch>"#;

        let mut info = UpdateInfo::empty(PlaformVariant::PS3);
        parse_response(xml, &mut info).unwrap();

        assert_eq!(info.title_id, "NPUB30826");
        assert_eq!(info.tag_name, "NPUB30826_T2");
        assert_eq!(info.titles, vec!["Some Game"]);
        assert_eq!(info.packages.len(), 2);
        assert_eq!(info.packages[0].size, 100);
        assert_eq!(info.packages[1].url, "http://a/y.pkg");
        assert_eq!(info.packages[1].manifest_url, "http://a/m.json");
    }

    #[test]
    fn returns_error_code() {
        let xml = "<Error><Code>NoSuchKey</Code></Error>";
        let mut info = UpdateInfo::empty(PlaformVariant::PS3);
        assert!(matches!(parse_response(xml, &mut info), Err(ParseError::ErrorCode(c)) if c == "NoSuchKey"));
    }
}
