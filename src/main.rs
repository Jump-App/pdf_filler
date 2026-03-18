mod fill;
mod strip;

use std::collections::HashMap;
use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() != 4 {
        eprintln!("Usage: pdf_filler <template.pdf> <data.json> <output.pdf>");
        process::exit(1);
    }

    let template_path = &args[1];
    let json_path = &args[2];
    let output_path = &args[3];

    let mut doc = lopdf::Document::load(template_path).unwrap_or_else(|e| {
        eprintln!("Failed to load PDF '{template_path}': {e}");
        process::exit(1);
    });

    // Optional: dump all field names the binary can see
    if env::var("PDF_DUMP_FIELDS").is_ok() {
        let field_names = fill::list_field_names(&doc);
        for name in &field_names {
            println!("{name}");
        }
        eprintln!("Found {} fields total", field_names.len());
        return;
    }

    let json_str = fs::read_to_string(json_path).unwrap_or_else(|e| {
        eprintln!("Failed to read JSON file '{json_path}': {e}");
        process::exit(1);
    });

    let raw: HashMap<String, serde_json::Value> =
        serde_json::from_str(&json_str).unwrap_or_else(|e| {
            eprintln!("Failed to parse JSON: {e}");
            process::exit(1);
        });

    // Convert all values to strings (handles integers, booleans, etc.)
    let data: HashMap<String, String> = raw
        .into_iter()
        .filter_map(|(k, v)| {
            let s = match &v {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Null => return None,
                _ => v.to_string(),
            };
            Some((k, s))
        })
        .collect();

    let filled_keys = fill::fill_pdf(&mut doc, &data);
    strip::strip_unfilled_dropdowns(&mut doc, &filled_keys);

    doc.save(output_path).unwrap_or_else(|e| {
        eprintln!("Failed to save PDF '{output_path}': {e}");
        process::exit(1);
    });

    eprintln!("Filled {} fields, wrote {}", filled_keys.len(), output_path);
}
