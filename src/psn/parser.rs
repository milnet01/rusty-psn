use log::warn;
use quick_xml::events::attributes::Attributes;
use quick_xml::events::Event;
use quick_xml::Reader;
use quick_xml::XmlVersion;

use super::{PackageInfo, UpdateInfo};

#[derive(Debug)]
pub enum ParseError {
    ErrorCode(String),
    XmlParsing(quick_xml::Error),
    InvalidTitleId(String),
}

fn parse_package_attributes(attributes: Attributes, info: &mut UpdateInfo) -> Result<(), ParseError> {
    for attribute in attributes.filter_map(|a| a.ok()) {
        match attribute.key.as_ref() {
            "version" => {
                let value = attribute
                    .normalized_value(XmlVersion::Implicit1_0)
                    .map_err(ParseError::XmlParsing)?;

                let mut package = PackageInfo::empty();
                package.version = value.to_string();

                info.packages.push(package);
            }
            "size" => {
                if let Some(last) = info.packages.last_mut() {
                    let value = attribute
                        .normalized_value(XmlVersion::Implicit1_0)
                        .map_err(ParseError::XmlParsing)?;
                    let parsed_value = value.parse::<u64>().unwrap_or_default();

                    last.size = parsed_value;
                }
            }
            "sha1sum" => {
                if let Some(last) = info.packages.last_mut() {
                    let value = attribute
                        .normalized_value(XmlVersion::Implicit1_0)
                        .map_err(ParseError::XmlParsing)?;
                    last.sha1sum = value.to_string();
                }
            }
            "url" => {
                if let Some(last) = info.packages.last_mut() {
                    let value = attribute
                        .normalized_value(XmlVersion::Implicit1_0)
                        .map_err(ParseError::XmlParsing)?;
                    last.url = value.to_string();
                }
            }
            "manifest_url" => {
                if let Some(last) = info.packages.last_mut() {
                    let value = attribute
                        .normalized_value(XmlVersion::Implicit1_0)
                        .map_err(ParseError::XmlParsing)?;
                    last.manifest_url = value.to_string();
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn parse_response(response: String, info: &mut UpdateInfo) -> Result<(), ParseError> {
    let mut reader = Reader::from_str(&response);
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
                    "titlepatch" => {
                        for attribute in e.attributes().filter_map(|a| a.ok()) {
                            if attribute.key.as_ref() == "titleid" {
                                if let Ok(value) = attribute.normalized_value(XmlVersion::Implicit1_0) {
                                    // The title id becomes a folder name, so it must be a valid serial.
                                    if crate::security::validate_serial(&value).is_err() {
                                        return Err(ParseError::InvalidTitleId(value.to_string()));
                                    }
                                    info.title_id = value.to_string();
                                }
                            }
                        }
                    }
                    "tag" => {
                        for attribute in e.attributes().filter_map(|a| a.ok()) {
                            if attribute.key.as_ref() == "name" {
                                if let Ok(value) = attribute.normalized_value(XmlVersion::Implicit1_0) {
                                    info.tag_name = value.to_string();
                                }
                            }
                        }
                    }
                    "package" => {
                        parse_package_attributes(e.attributes(), info)?;
                    }
                    "Error" => {
                        err_encountered = true;
                    }
                    "Code" => {
                        if !err_encountered {
                            warn!("Code tag encountered without a preceeding Error tag, skipping it");
                            continue;
                        }

                        err_code_encountered = true;
                    }
                    _ => {
                        if e.name().as_ref().to_lowercase().starts_with("title") {
                            title_element = true;
                        }
                    }
                }
            }
            Ok(Event::End(_)) => {
                depth -= 1;
            }
            Ok(Event::Empty(e)) => {
                if let "package" = e.name().as_ref() {
                    parse_package_attributes(e.attributes(), info)?;
                }
            }
            Ok(Event::Text(e)) => {
                if title_element {
                    let title = e.xml10_content();

                    title_element = false;
                    info.titles.push(title.to_string());
                } else if err_code_encountered {
                    let err_code_text = e.xml10_content();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::psn::PlatformVariant;

    #[test]
    fn parse_response_rejects_unsafe_title_id() {
        // The title id in Sony's reply becomes a folder name, so it must be a valid serial.
        let xml = r#"<titlepatch titleid="../../evil"><tag name="x"><package version="01.01" size="1" sha1sum="a" url="https://a0.ww.np.dl.playstation.net/x.pkg"/></tag></titlepatch>"#;
        let mut info = UpdateInfo::empty(PlatformVariant::PS3);
        assert!(parse_response(xml.to_string(), &mut info).is_err());
    }
}
