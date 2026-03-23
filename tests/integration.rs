mod common;

use std::process::Command;
use tempfile::TempDir;

/// Helper: save the test PDF and JSON data to a temp dir, run the binary, return output.
fn run_pdf_filler(
    json_content: &str,
    env_vars: &[(&str, &str)],
) -> (std::process::Output, TempDir) {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let (mut doc, _ids) = common::create_test_pdf();

    let template_path = tmp.path().join("template.pdf");
    let json_path = tmp.path().join("data.json");
    let output_path = tmp.path().join("output.pdf");

    doc.save(&template_path).expect("failed to save test PDF");
    std::fs::write(&json_path, json_content).expect("failed to write JSON");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_pdf_filler"));
    cmd.arg(&template_path).arg(&json_path).arg(&output_path);

    for (key, val) in env_vars {
        cmd.env(key, val);
    }

    let output = cmd.output().expect("failed to execute binary");
    (output, tmp)
}

#[test]
fn e2e_fills_pdf_successfully() {
    let json = r#"{
        "Name": "Jane Doe",
        "Agree": "Yes",
        "State": "NY",
        "Account.Number": "12345"
    }"#;

    let (output, tmp) = run_pdf_filler(json, &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "binary should exit 0, stderr: {stderr}"
    );
    assert!(
        stderr.contains("Filled"),
        "stderr should mention filled fields: {stderr}"
    );

    // Verify the output PDF exists and can be loaded
    let output_path = tmp.path().join("output.pdf");
    assert!(output_path.exists(), "output PDF should exist");

    let doc = lopdf::Document::load(&output_path).expect("output PDF should be loadable");

    // Verify at least one field was filled by checking the document is valid
    assert!(
        doc.catalog().is_ok(),
        "output PDF should have a valid catalog"
    );
}

#[test]
fn e2e_null_values_skipped() {
    let json = r#"{
        "Name": "Jane Doe",
        "Agree": null,
        "State": null
    }"#;

    let (output, _tmp) = run_pdf_filler(json, &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "binary should exit 0, stderr: {stderr}"
    );
    // Only "Name" should be filled (null values are skipped by main.rs filter_map)
    assert!(
        stderr.contains("Filled 1 fields"),
        "should fill exactly 1 field, stderr: {stderr}"
    );
}

#[test]
fn e2e_dump_fields_mode() {
    let json = "{}"; // doesn't matter for dump mode

    let (output, _tmp) = run_pdf_filler(json, &[("PDF_DUMP_FIELDS", "1")]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "binary should exit 0, stderr: {stderr}"
    );

    // Should list field names on stdout
    assert!(
        stdout.contains("Name"),
        "stdout should contain 'Name': {stdout}"
    );
    assert!(
        stdout.contains("State"),
        "stdout should contain 'State': {stdout}"
    );
    assert!(
        stdout.contains("Account"),
        "stdout should contain 'Account': {stdout}"
    );

    // stderr should have total count
    assert!(
        stderr.contains("fields total"),
        "stderr should mention total: {stderr}"
    );
}

#[test]
fn e2e_wrong_args_exits_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_pdf_filler"))
        .output()
        .expect("failed to execute binary");

    assert!(
        !output.status.success(),
        "binary should exit non-zero with no args"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Usage:"),
        "stderr should contain usage message: {stderr}"
    );
}

#[test]
fn e2e_filled_fields_have_appearance_streams() {
    let json = r#"{
        "Name": "Jane Doe",
        "State": "NY",
        "Account.Number": "12345"
    }"#;

    let (output, tmp) = run_pdf_filler(json, &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "binary should exit 0, stderr: {stderr}"
    );

    let output_path = tmp.path().join("output.pdf");
    let doc = lopdf::Document::load(&output_path).expect("output PDF should be loadable");

    // Check that filled text/choice fields have /AP → /N
    let pages: Vec<lopdf::ObjectId> = doc.page_iter().collect();
    let page = doc.get_object(pages[0]).unwrap().as_dict().unwrap();
    let annots_obj = page.get(b"Annots").unwrap();
    let annots = match annots_obj {
        lopdf::Object::Array(arr) => arr.clone(),
        lopdf::Object::Reference(id) => doc.get_object(*id).unwrap().as_array().unwrap().clone(),
        _ => panic!("unexpected Annots type"),
    };

    let mut found_name = false;
    for annot_ref in &annots {
        if let Ok(id) = annot_ref.as_reference() {
            if let Ok(obj) = doc.get_object(id) {
                if let Ok(dict) = obj.as_dict() {
                    if let Ok(lopdf::Object::String(bytes, _)) = dict.get(b"T") {
                        let name = String::from_utf8_lossy(bytes);
                        if name == "Name" {
                            found_name = true;
                            let ap = dict
                                .get(b"AP")
                                .expect("Name field should have /AP");
                            let ap_dict = ap.as_dict().expect("/AP should be a dict");
                            let n = ap_dict
                                .get(b"N")
                                .expect("/AP should have /N entry");
                            let n_id = n
                                .as_reference()
                                .expect("/AP /N should be a reference");
                            let stream_obj = doc
                                .get_object(n_id)
                                .expect("AP stream object should exist");
                            assert!(
                                stream_obj.as_stream().is_ok(),
                                "/AP /N should reference a stream"
                            );
                        }
                    }
                }
            }
        }
    }

    assert!(found_name, "Name field should be found in annotations");
}

#[test]
fn e2e_strips_unfilled_dropdown() {
    // Fill Name but NOT State — State's DV/V should be stripped
    let json = r#"{ "Name": "Alice" }"#;

    let (output, tmp) = run_pdf_filler(json, &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "binary should exit 0, stderr: {stderr}"
    );

    let output_path = tmp.path().join("output.pdf");
    let doc = lopdf::Document::load(&output_path).expect("output PDF should be loadable");

    // Find the State field and verify DV/V were stripped
    let pages: Vec<lopdf::ObjectId> = doc.page_iter().collect();
    assert!(!pages.is_empty(), "should have at least one page");

    let page = doc.get_object(pages[0]).unwrap().as_dict().unwrap();
    if let Ok(annots_obj) = page.get(b"Annots") {
        let annots = match annots_obj {
            lopdf::Object::Array(arr) => arr.clone(),
            lopdf::Object::Reference(id) => {
                doc.get_object(*id).unwrap().as_array().unwrap().clone()
            }
            _ => panic!("unexpected Annots type"),
        };

        for annot_ref in &annots {
            if let Ok(id) = annot_ref.as_reference() {
                if let Ok(obj) = doc.get_object(id) {
                    if let Ok(dict) = obj.as_dict() {
                        if let Ok(lopdf::Object::String(bytes, _)) = dict.get(b"T") {
                            if bytes == b"State" {
                                assert!(dict.get(b"DV").is_err(), "State DV should be stripped");
                                assert!(dict.get(b"V").is_err(), "State V should be stripped");
                                return;
                            }
                        }
                    }
                }
            }
        }
    }

    panic!("State field not found in output PDF annotations");
}
