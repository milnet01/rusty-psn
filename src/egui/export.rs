//! Saving the search results to a file, as CSV for a spreadsheet or JSON for other tools
//! (RPSN-0071). One row per package.

use super::*;

/// One package of one search result, as it is written out.
#[derive(Serialize)]
struct ExportRow {
    serial: String,
    title: String,
    platform: String,
    version: String,
    size_bytes: u64,
    sha1: String,
    url: String,
}

fn rows(results: &[UpdateInfo]) -> Vec<ExportRow> {
    results
        .iter()
        .flat_map(|update| {
            update.packages.iter().map(move |pkg| ExportRow {
                serial: update.title_id.clone(),
                title: update.title(),
                platform: update.platform_variant.to_string(),
                version: pkg.id(),
                size_bytes: pkg.size,
                sha1: pkg.sha1sum.clone(),
                url: pkg.url.clone(),
            })
        })
        .collect()
}

/// A CSV cell, quoted where needed. A cell a spreadsheet would read as a formula gets a leading
/// apostrophe, so a title such as "=HYPERLINK(...)" stays text (CSV injection).
fn csv_cell(text: &str) -> String {
    let text = if text.starts_with(['=', '+', '-', '@']) {
        format!("'{text}")
    } else {
        text.to_string()
    };
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}

fn to_csv(rows: &[ExportRow]) -> String {
    let mut out = String::from("serial,title,platform,version,size_bytes,sha1,url\n");
    for row in rows {
        let cells = [
            csv_cell(&row.serial),
            csv_cell(&row.title),
            csv_cell(&row.platform),
            csv_cell(&row.version),
            row.size_bytes.to_string(),
            csv_cell(&row.sha1),
            csv_cell(&row.url),
        ];
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    out
}

/// The file's contents: JSON when the name ends in `.json`, CSV otherwise.
fn export_text(results: &[UpdateInfo], path: &Path) -> Result<String, serde_json::Error> {
    let rows = rows(results);
    let json = path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("json"));
    if json {
        serde_json::to_string_pretty(&rows)
    } else {
        Ok(to_csv(&rows))
    }
}

impl UpdatesApp {
    /// Ask where to save the results with the desktop's own file picker, then write them.
    pub(super) fn export_results(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_file_name("rusty-psn-results.csv")
            .add_filter("CSV (spreadsheet)", &["csv"])
            .add_filter("JSON", &["json"])
            .save_file()
        else {
            return;
        };
        let written = export_text(&self.v.update_results, &path)
            .map_err(|e| e.to_string())
            .and_then(|text| std::fs::write(&path, text).map_err(|e| e.to_string()));
        match written {
            Ok(()) => self.show_notifications(format!("Results saved to {}", path.display()), MessageLevel::Success),
            Err(e) => {
                error!("Could not save results to {}: {e}", path.display());
                self.show_notifications(format!("Could not save the results: {e}"), MessageLevel::Error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(title: &str) -> UpdateInfo {
        UpdateInfo {
            title_id: "BLES00001".to_string(),
            tag_name: String::new(),
            titles: vec![title.to_string()],
            packages: vec![PackageInfo {
                url: "http://b0.ww.np.dl.playstation.net/a.pkg".to_string(),
                size: 1024,
                version: "01.02".to_string(),
                sha1sum: "abc".to_string(),
                hash_whole_file: false,
                manifest_url: String::new(),
                offset: 0,
                part_number: None,
                min_system_version: None,
            }],
            platform_variant: crate::psn::utils::PlatformVariant::PS3,
        }
    }

    #[test]
    fn csv_has_one_row_per_package_and_quotes_where_needed() {
        let csv = export_text(&[result("Ratchet & Clank, \"Future\"")], Path::new("out.csv")).expect("CSV builds");
        let lines: Vec<_> = csv.lines().collect();
        assert_eq!(lines[0], "serial,title,platform,version,size_bytes,sha1,url");
        assert_eq!(
            lines[1],
            "BLES00001,\"Ratchet & Clank, \"\"Future\"\"\",PS3,01.02,1024,abc,http://b0.ww.np.dl.playstation.net/a.pkg"
        );
    }

    #[test]
    fn a_formula_title_stays_text_in_csv() {
        let csv = export_text(&[result("=HYPERLINK(\"x\")")], Path::new("out.csv")).expect("CSV builds");
        assert!(
            csv.lines()
                .nth(1)
                .is_some_and(|row| row.contains("\"'=HYPERLINK(\"\"x\"\")\"")),
            "{csv}"
        );
    }

    #[test]
    fn a_json_name_gives_json() {
        let json = export_text(&[result("Game")], Path::new("out.JSON")).expect("JSON builds");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("the export is valid JSON");
        assert_eq!(parsed[0]["serial"], "BLES00001");
        assert_eq!(parsed[0]["size_bytes"], 1024);
    }
}
