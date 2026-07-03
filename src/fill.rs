use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use std::collections::{HashMap, HashSet};

/// Fill PDF form fields from a flat key→value map.
/// Returns the set of field names that were successfully filled.
pub fn fill_pdf(doc: &mut Document, data: &HashMap<String, String>) -> HashSet<String> {
    let mut filled = HashSet::new();

    // Collect all field references from the AcroForm tree
    let field_ids = collect_field_ids(doc);

    for field_id in field_ids {
        if let Some(field_name) = get_field_name(doc, field_id) {
            if let Some(value) = data.get(&field_name) {
                if !value.is_empty() {
                    set_field_value(doc, field_id, &field_name, value);
                    let field_type = get_field_type(doc, field_id);
                    if field_type.as_deref() != Some("Btn") {
                        generate_appearance_stream(doc, field_id, value);
                    }
                    filled.insert(field_name);
                }
            }
        }
    }

    // Also fill page annotation widgets. Some PDFs have separate AcroForm field
    // dicts and page annotation widgets with the same name. We must update both
    // so the /V value and /AP appearance are on the objects the page actually renders.
    let annot_ids = collect_page_annotation_ids(doc);
    for annot_id in annot_ids {
        if let Some(field_name) = get_field_name(doc, annot_id) {
            if let Some(value) = data.get(&field_name) {
                if !value.is_empty() {
                    set_field_value(doc, annot_id, &field_name, value);
                    let field_type = get_field_type(doc, annot_id);
                    if field_type.as_deref() != Some("Btn") {
                        generate_appearance_stream(doc, annot_id, value);
                    }
                    filled.insert(field_name);
                }
            }
        }
    }

    // Point every text/choice /DA at the standard Helvetica so Adobe's NeedAppearances
    // regeneration never resolves the template's "ArialMT" (which has no valid font and
    // triggers a "bad /BBox" warning).
    normalize_text_das(doc);

    // Keep NeedAppearances=true. This template is malformed — each field exists as two
    // unlinked widgets (an off-page AcroForm entry and the on-page annotation) — and
    // Adobe only renders it correctly by merging same-named widgets and rebuilding each
    // field from its value on open, which NeedAppearances triggers. That rebuild is what
    // resolves checkbox on/off state; dropping the flag left checkboxes blank in Adobe.
    // It's safe to keep because we regenerate the /AP streams above, so viewers that
    // ignore NeedAppearances (Preview/Chrome) still paint the correct values.
    set_need_appearances(doc);

    filled
}

/// Rewrite the /DA of every text/choice widget that has its own /DA to the standard-14
/// Helvetica ("/Helv"), preserving the font size. Kills all references to the template's
/// "ArialMT", which has no valid font object and would make Adobe warn "bad /BBox".
fn normalize_text_das(doc: &mut Document) {
    let mut ids = collect_field_ids(doc);
    ids.extend(collect_page_annotation_ids(doc));

    for id in ids {
        let is_text = matches!(get_field_type(doc, id).as_deref(), Some("Tx") | Some("Ch"));
        // The field's own /DA (not inherited).
        let own_da = doc
            .get_object(id)
            .ok()
            .and_then(|o| o.as_dict().ok())
            .and_then(|d| d.get(b"DA").ok())
            .and_then(|o| pdf_string_to_rust(o).ok());
        let own_da = match own_da {
            Some(da) => da,
            None => continue,
        };
        // Normalize every text/choice /DA, plus any /DA that references the template's
        // invalid ArialMT (e.g. a pushbutton caption font).
        if !is_text && !own_da.contains("ArialMT") {
            continue;
        }
        let size = parse_font_size_from_da(&own_da).unwrap_or(0.0);
        if let Ok(obj) = doc.get_object_mut(id) {
            if let Ok(dict) = obj.as_dict_mut() {
                dict.set(
                    "DA",
                    Object::String(
                        format!("/Helv {size:.2} Tf 0 g").into_bytes(),
                        StringFormat::Literal,
                    ),
                );
            }
        }
    }
}

/// List all field names found in the PDF (AcroForm tree + page annotations).
pub fn list_field_names(doc: &Document) -> Vec<String> {
    let mut all_ids = collect_field_ids(doc);
    all_ids.extend(collect_page_annotation_ids(doc));
    let mut names: Vec<String> = all_ids
        .into_iter()
        .filter_map(|id| get_field_name(doc, id))
        .collect();
    names.sort();
    names.dedup();
    names
}

pub(crate) fn set_need_appearances(doc: &mut Document) {
    let catalog = doc.catalog().expect("No catalog found").clone();

    if let Ok(acroform_ref) = catalog.get(b"AcroForm") {
        if let Ok(acroform_id) = acroform_ref.as_reference() {
            if let Ok(obj) = doc.get_object_mut(acroform_id) {
                if let Ok(dict) = obj.as_dict_mut() {
                    dict.set("NeedAppearances", Object::Boolean(true));
                }
            }
        }
    }
}

pub(crate) fn collect_field_ids(doc: &Document) -> Vec<ObjectId> {
    let mut ids = Vec::new();
    let catalog = doc.catalog().expect("No catalog").clone();

    if let Ok(acroform_ref) = catalog.get(b"AcroForm") {
        let acroform = resolve_dict(doc, acroform_ref);
        if let Some(dict) = acroform {
            if let Ok(fields) = dict.get(b"Fields") {
                if let Ok(arr) = resolve_object(doc, fields).as_array() {
                    for field_ref in arr {
                        if let Ok(id) = field_ref.as_reference() {
                            collect_field_tree(doc, id, &mut ids);
                        }
                    }
                }
            }
        }
    }

    ids
}

fn collect_field_tree(doc: &Document, id: ObjectId, ids: &mut Vec<ObjectId>) {
    ids.push(id);

    if let Ok(obj) = doc.get_object(id) {
        if let Ok(dict) = obj.as_dict() {
            if let Ok(kids) = dict.get(b"Kids") {
                if let Ok(arr) = resolve_object(doc, kids).as_array() {
                    for kid_ref in arr {
                        if let Ok(kid_id) = kid_ref.as_reference() {
                            collect_field_tree(doc, kid_id, ids);
                        }
                    }
                }
            }
        }
    }
}

/// Collect annotation object IDs from all pages (catches orphan widget fields).
pub(crate) fn collect_page_annotation_ids(doc: &Document) -> Vec<ObjectId> {
    let mut ids = Vec::new();
    let page_ids: Vec<ObjectId> = doc.page_iter().collect();

    for page_id in page_ids {
        if let Ok(obj) = doc.get_object(page_id) {
            if let Ok(dict) = obj.as_dict() {
                if let Ok(annots_obj) = dict.get(b"Annots") {
                    let annots = resolve_object(doc, annots_obj);
                    if let Ok(arr) = annots.as_array() {
                        for item in arr {
                            if let Ok(id) = item.as_reference() {
                                ids.push(id);
                            }
                        }
                    }
                }
            }
        }
    }

    ids
}

pub(crate) fn get_field_name(doc: &Document, id: ObjectId) -> Option<String> {
    let mut parts = Vec::new();
    let mut current_id = Some(id);

    while let Some(cid) = current_id {
        if let Ok(obj) = doc.get_object(cid) {
            if let Ok(dict) = obj.as_dict() {
                if let Ok(t) = dict.get(b"T") {
                    if let Ok(name) = pdf_string_to_rust(t) {
                        parts.push(name);
                    }
                }
                current_id = dict.get(b"Parent").ok().and_then(|p| p.as_reference().ok());
            } else {
                break;
            }
        } else {
            break;
        }
    }

    if parts.is_empty() {
        return None;
    }

    parts.reverse();
    Some(parts.join("."))
}

/// Encode a field value as a PDF text string. ASCII stays a plain literal; anything
/// else becomes UTF-16BE with a BOM so a programmatic reader decodes the exact value
/// (a raw-UTF-8 literal would be misread as Latin-1, e.g. "José" -> "JosÃ©").
fn pdf_text_string(value: &str) -> Object {
    if value.is_ascii() {
        Object::String(value.as_bytes().to_vec(), StringFormat::Literal)
    } else {
        let mut bytes = vec![0xFE, 0xFF];
        for unit in value.encode_utf16() {
            bytes.extend_from_slice(&unit.to_be_bytes());
        }
        Object::String(bytes, StringFormat::Hexadecimal)
    }
}

pub(crate) fn set_field_value(doc: &mut Document, id: ObjectId, _name: &str, value: &str) {
    let field_type = get_field_type(doc, id);

    if let Ok(obj) = doc.get_object_mut(id) {
        if let Ok(dict) = obj.as_dict_mut() {
            match field_type.as_deref() {
                Some("Btn") => {
                    // Button on-states are name tokens (e.g. /Yes), always ASCII.
                    dict.set("V", Object::Name(value.as_bytes().to_vec()));
                    dict.set("AS", Object::Name(value.as_bytes().to_vec()));
                }
                // Text and choice: store the exact value as a PDF text string.
                _ => {
                    dict.set("V", pdf_text_string(value));
                }
            }
        }
    }
}

pub(crate) fn get_field_type(doc: &Document, id: ObjectId) -> Option<String> {
    let mut current_id = Some(id);
    while let Some(cid) = current_id {
        if let Ok(obj) = doc.get_object(cid) {
            if let Ok(dict) = obj.as_dict() {
                if let Ok(ft) = dict.get(b"FT") {
                    if let Ok(name) = ft.as_name_str() {
                        return Some(name.to_string());
                    }
                }
                current_id = dict.get(b"Parent").ok().and_then(|p| p.as_reference().ok());
            } else {
                break;
            }
        } else {
            break;
        }
    }
    None
}

pub(crate) fn resolve_object<'a>(doc: &'a Document, obj: &'a Object) -> &'a Object {
    match obj {
        Object::Reference(id) => doc.get_object(*id).unwrap_or(obj),
        _ => obj,
    }
}

pub(crate) fn resolve_dict<'a>(
    doc: &'a Document,
    obj: &'a Object,
) -> Option<&'a lopdf::Dictionary> {
    match obj {
        Object::Reference(id) => doc.get_object(*id).ok().and_then(|o| o.as_dict().ok()),
        Object::Dictionary(d) => Some(d),
        _ => None,
    }
}

/// Escape special PDF string characters: \, (, )
/// PDF text-field flag: multiline (bit 13).
const FLAG_MULTILINE: i64 = 0x1000;

/// Resolve a field's /Ff flags, inheriting from /Parent.
fn get_field_flags(doc: &Document, id: ObjectId) -> i64 {
    let mut current = Some(id);
    while let Some(cid) = current {
        let dict = match doc.get_object(cid).ok().and_then(|o| o.as_dict().ok()) {
            Some(d) => d,
            None => break,
        };
        if let Ok(Object::Integer(n)) = dict.get(b"Ff") {
            return *n;
        }
        current = dict.get(b"Parent").ok().and_then(|p| p.as_reference().ok());
    }
    0
}

/// Escape special characters for a PDF literal string.
fn pdf_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            _ => out.push(c),
        }
    }
    out
}

/// Greedy word-wrap into lines that fit `avail_width` at `font_size`, using an average
/// glyph advance (no embedded metrics, so this is approximate). Splits on explicit
/// newlines.
fn wrap_text(value: &str, avail_width: f64, font_size: f64) -> Vec<String> {
    let char_width = (0.5 * font_size).max(1.0);
    let max_chars = ((avail_width / char_width).floor() as usize).max(1);

    let mut lines = Vec::new();
    for paragraph in value.split('\n') {
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            // Wrap only at word boundaries — never split a word. A word longer than the
            // line goes on its own line and overflows (clipped), so text is never altered.
            if current.is_empty() {
                current = word.to_string();
            } else if current.chars().count() + 1 + word.chars().count() <= max_chars {
                current.push(' ');
                current.push_str(word);
            } else {
                lines.push(std::mem::take(&mut current));
                current = word.to_string();
            }
        }
        lines.push(current);
    }
    lines
}

/// Get the /DA (Default Appearance) string, walking up the parent chain,
/// falling back to AcroForm /DA.
fn get_da_string(doc: &Document, id: ObjectId) -> Option<String> {
    // Walk field hierarchy
    let mut current_id = Some(id);
    while let Some(cid) = current_id {
        if let Ok(obj) = doc.get_object(cid) {
            if let Ok(dict) = obj.as_dict() {
                if let Ok(da) = dict.get(b"DA") {
                    if let Ok(s) = pdf_string_to_rust(da) {
                        return Some(s);
                    }
                }
                current_id = dict.get(b"Parent").ok().and_then(|p| p.as_reference().ok());
            } else {
                break;
            }
        } else {
            break;
        }
    }

    // Fallback: AcroForm /DA
    let catalog = doc.catalog().ok()?.clone();
    let acroform_ref = catalog.get(b"AcroForm").ok()?;
    let acroform = resolve_dict(doc, acroform_ref)?;
    let da = acroform.get(b"DA").ok()?;
    pdf_string_to_rust(da).ok()
}

/// Get the /Rect array [x1, y1, x2, y2] from a field widget.
fn get_field_rect(doc: &Document, id: ObjectId) -> Option<[f64; 4]> {
    let obj = doc.get_object(id).ok()?;
    let dict = obj.as_dict().ok()?;
    let rect_obj = resolve_object(doc, dict.get(b"Rect").ok()?);
    let arr = rect_obj.as_array().ok()?;
    if arr.len() < 4 {
        return None;
    }

    let mut vals = [0.0f64; 4];
    for (i, item) in arr.iter().take(4).enumerate() {
        vals[i] = match item {
            Object::Real(f) => *f as f64,
            Object::Integer(n) => *n as f64,
            _ => return None,
        };
    }
    Some(vals)
}

/// Parse font name from DA string. E.g. "/Helv 12 Tf 0 g" → "Helv"
/// Parse font size from DA string. E.g. "/Helv 12 Tf 0 g" → 12.0
fn parse_font_size_from_da(da: &str) -> Option<f64> {
    for (i, token) in da.split_whitespace().enumerate() {
        if token == "Tf" && i >= 1 {
            let tokens: Vec<&str> = da.split_whitespace().collect();
            return tokens[i - 1].parse::<f64>().ok();
        }
    }
    None
}

/// Generate an /AP appearance stream for a text or choice field.
fn generate_appearance_stream(doc: &mut Document, id: ObjectId, value: &str) {
    let rect = match get_field_rect(doc, id) {
        Some(r) => r,
        None => return, // No rect, can't generate appearance
    };

    let da = match get_da_string(doc, id) {
        Some(s) => s,
        None => return, // No DA, can't generate appearance
    };

    let width = (rect[2] - rect[0]).abs();
    let height = (rect[3] - rect[1]).abs();

    let font_size = parse_font_size_from_da(&da).unwrap_or(12.0);
    let effective_size = if font_size <= 0.0 { 12.0 } else { font_size };

    let pad = 2.0;
    // Multiline fields (or values with explicit newlines) wrap to the box width and
    // render top-down; everything else is a single vertically-centered line.
    let multiline = get_field_flags(doc, id) & FLAG_MULTILINE != 0 || value.contains('\n');

    // Draw with a standard-14 Helvetica (declared in the resources below) — NOT the
    // field's /DA font. The template's choice fields name "ArialMT", and a Type1 font
    // with a non-standard BaseFont and no FontDescriptor makes Adobe warn "bad /BBox".
    let font_ops = format!("/Helv {effective_size:.2} Tf 0 g");

    let content = if multiline {
        let avail = (width - 2.0 * pad).max(1.0);
        let leading = effective_size * 1.15;
        let max_lines = (((height - pad) / leading).floor() as usize).max(1);
        let mut lines = wrap_text(value, avail, effective_size);
        lines.truncate(max_lines);
        let first_baseline = (height - pad - effective_size).max(0.0);
        let mut ops =
            format!("/Tx BMC\nBT\n{font_ops}\n{leading:.2} TL\n{pad:.2} {first_baseline:.2} Td\n");
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                ops.push_str("T*\n");
            }
            ops.push_str(&format!("({}) Tj\n", pdf_escape(line)));
        }
        ops.push_str("ET\nEMC");
        ops
    } else {
        let baseline = {
            let centered = (height - effective_size) / 2.0;
            if centered < 0.0 {
                pad
            } else {
                centered
            }
        };
        format!(
            "/Tx BMC\nBT\n{font_ops}\n{pad:.2} {baseline:.2} Td\n({}) Tj\nET\nEMC",
            pdf_escape(value)
        )
    };

    // Self-contained standard Helvetica: standard-14, so it needs no FontDescriptor or
    // FontBBox and never triggers the "bad /BBox" warning.
    let mut helvetica = Dictionary::new();
    helvetica.set("Type", Object::Name(b"Font".to_vec()));
    helvetica.set("Subtype", Object::Name(b"Type1".to_vec()));
    helvetica.set("BaseFont", Object::Name(b"Helvetica".to_vec()));
    let mut fonts = Dictionary::new();
    fonts.set("Helv", Object::Dictionary(helvetica));
    let mut resources = Dictionary::new();
    resources.set("Font", Object::Dictionary(fonts));

    // Build the Form XObject stream
    let mut stream_dict = Dictionary::from_iter(vec![
        (b"Type".to_vec(), Object::Name(b"XObject".to_vec())),
        (b"Subtype".to_vec(), Object::Name(b"Form".to_vec())),
        (
            b"BBox".to_vec(),
            Object::Array(vec![
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(width as f32),
                Object::Real(height as f32),
            ]),
        ),
        (b"Resources".to_vec(), Object::Dictionary(resources)),
    ]);

    let content_bytes = content.into_bytes();
    stream_dict.set("Length", Object::Integer(content_bytes.len() as i64));

    let stream = Stream::new(stream_dict, content_bytes);
    let ap_id = doc.add_object(Object::Stream(stream));

    // Set /AP on the field, and point /DA at the same standard font so Adobe's
    // NeedAppearances rebuild uses Helvetica instead of the unresolved/invalid ArialMT.
    if let Ok(obj) = doc.get_object_mut(id) {
        if let Ok(dict) = obj.as_dict_mut() {
            let ap_dict = Dictionary::from_iter(vec![(b"N".to_vec(), Object::Reference(ap_id))]);
            dict.set("AP", Object::Dictionary(ap_dict));
            dict.set(
                "DA",
                Object::String(
                    format!("/Helv {effective_size:.2} Tf 0 g").into_bytes(),
                    StringFormat::Literal,
                ),
            );
        }
    }
}

pub(crate) fn pdf_string_to_rust(obj: &Object) -> Result<String, ()> {
    match obj {
        Object::String(bytes, _) => {
            if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
                let utf16: Vec<u16> = bytes[2..]
                    .chunks(2)
                    .map(|c| {
                        if c.len() == 2 {
                            u16::from_be_bytes([c[0], c[1]])
                        } else {
                            0
                        }
                    })
                    .collect();
                String::from_utf16(&utf16).map_err(|_| ())
            } else {
                Ok(String::from_utf8_lossy(bytes).into_owned())
            }
        }
        Object::Name(bytes) => Ok(String::from_utf8_lossy(bytes).into_owned()),
        _ => Err(()),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use lopdf::dictionary;
    use lopdf::{Document, Object, ObjectId, StringFormat};
    use std::collections::HashMap;

    /// IDs returned by `create_test_pdf` so tests can reference specific objects.
    #[allow(dead_code)]
    pub(crate) struct TestPdfIds {
        pub catalog_id: ObjectId,
        pub acroform_id: ObjectId,
        pub page_id: ObjectId,
        pub pages_id: ObjectId,
        pub name_field_id: ObjectId,
        pub agree_field_id: ObjectId,
        pub state_field_id: ObjectId,
        pub account_parent_id: ObjectId,
        pub account_number_id: ObjectId,
        pub orphan_field_id: ObjectId,
    }

    /// Build a minimal in-memory PDF with various field types for testing.
    pub(crate) fn create_test_pdf() -> (Document, TestPdfIds) {
        let mut doc = Document::with_version("1.5");

        // --- Field objects ---

        // --- Font resource (minimal Helv placeholder) ---
        let helv_font_id = doc.add_object(Object::Dictionary(dictionary! {
            "Type" => Object::Name(b"Font".to_vec()),
            "Subtype" => Object::Name(b"Type1".to_vec()),
            "BaseFont" => Object::Name(b"Helvetica".to_vec()),
        }));

        // Text field "Name" (FT=Tx) with /DA and /Rect
        let name_field_id = doc.add_object(Object::Dictionary(dictionary! {
            "Type" => Object::Name(b"Annot".to_vec()),
            "Subtype" => Object::Name(b"Widget".to_vec()),
            "FT" => Object::Name(b"Tx".to_vec()),
            "T" => Object::String(b"Name".to_vec(), StringFormat::Literal),
            "DA" => Object::String(b"/Helv 12 Tf 0 g".to_vec(), StringFormat::Literal),
            "Rect" => Object::Array(vec![
                Object::Real(50.0), Object::Real(700.0),
                Object::Real(250.0), Object::Real(720.0),
            ]),
        }));

        // Button field "Agree" (FT=Btn)
        let agree_field_id = doc.add_object(Object::Dictionary(dictionary! {
            "Type" => Object::Name(b"Annot".to_vec()),
            "Subtype" => Object::Name(b"Widget".to_vec()),
            "FT" => Object::Name(b"Btn".to_vec()),
            "T" => Object::String(b"Agree".to_vec(), StringFormat::Literal),
        }));

        // Choice field "State" (FT=Ch) with DV and V defaults, /DA and /Rect
        let state_field_id = doc.add_object(Object::Dictionary(dictionary! {
            "Type" => Object::Name(b"Annot".to_vec()),
            "Subtype" => Object::Name(b"Widget".to_vec()),
            "FT" => Object::Name(b"Ch".to_vec()),
            "T" => Object::String(b"State".to_vec(), StringFormat::Literal),
            "DV" => Object::String(b"CA".to_vec(), StringFormat::Literal),
            "V" => Object::String(b"CA".to_vec(), StringFormat::Literal),
            "DA" => Object::String(b"/Helv 10 Tf 0 g".to_vec(), StringFormat::Literal),
            "Rect" => Object::Array(vec![
                Object::Real(50.0), Object::Real(650.0),
                Object::Real(200.0), Object::Real(670.0),
            ]),
        }));

        // Hierarchical: parent "Account" with child "Number" (FT=Tx)
        // Child needs a Parent reference, so we create parent first as a placeholder
        let account_parent_id = doc.add_object(Object::Dictionary(dictionary! {
            "T" => Object::String(b"Account".to_vec(), StringFormat::Literal),
        }));

        let account_number_id = doc.add_object(Object::Dictionary(dictionary! {
            "Type" => Object::Name(b"Annot".to_vec()),
            "Subtype" => Object::Name(b"Widget".to_vec()),
            "FT" => Object::Name(b"Tx".to_vec()),
            "T" => Object::String(b"Number".to_vec(), StringFormat::Literal),
            "Parent" => Object::Reference(account_parent_id),
            "DA" => Object::String(b"/Helv 12 Tf 0 g".to_vec(), StringFormat::Literal),
            "Rect" => Object::Array(vec![
                Object::Real(50.0), Object::Real(600.0),
                Object::Real(250.0), Object::Real(620.0),
            ]),
        }));

        // Update parent to have Kids array pointing to child
        doc.set_object(
            account_parent_id,
            Object::Dictionary(dictionary! {
                "T" => Object::String(b"Account".to_vec(), StringFormat::Literal),
                "Kids" => Object::Array(vec![Object::Reference(account_number_id)]),
            }),
        );

        // Orphan annotation — on page Annots but NOT in AcroForm Fields
        let orphan_field_id = doc.add_object(Object::Dictionary(dictionary! {
            "Type" => Object::Name(b"Annot".to_vec()),
            "Subtype" => Object::Name(b"Widget".to_vec()),
            "FT" => Object::Name(b"Tx".to_vec()),
            "T" => Object::String(b"OrphanField".to_vec(), StringFormat::Literal),
            "DA" => Object::String(b"/Helv 12 Tf 0 g".to_vec(), StringFormat::Literal),
            "Rect" => Object::Array(vec![
                Object::Real(50.0), Object::Real(550.0),
                Object::Real(250.0), Object::Real(570.0),
            ]),
        }));

        // --- Page ---
        let pages_id = doc.new_object_id();

        let page_id = doc.add_object(Object::Dictionary(dictionary! {
            "Type" => Object::Name(b"Page".to_vec()),
            "Parent" => Object::Reference(pages_id),
            "Annots" => Object::Array(vec![
                Object::Reference(name_field_id),
                Object::Reference(agree_field_id),
                Object::Reference(state_field_id),
                Object::Reference(orphan_field_id),
            ]),
        }));

        // Pages node
        doc.set_object(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => Object::Name(b"Pages".to_vec()),
                "Count" => Object::Integer(1),
                "Kids" => Object::Array(vec![Object::Reference(page_id)]),
            }),
        );

        // --- AcroForm with /DA and /DR (Default Resources) ---
        let font_dict = dictionary! {
            "Helv" => Object::Reference(helv_font_id),
        };
        let dr_dict = dictionary! {
            "Font" => Object::Dictionary(font_dict),
        };
        let acroform_id = doc.add_object(Object::Dictionary(dictionary! {
            "Fields" => Object::Array(vec![
                Object::Reference(name_field_id),
                Object::Reference(agree_field_id),
                Object::Reference(state_field_id),
                Object::Reference(account_parent_id),
            ]),
            "DA" => Object::String(b"/Helv 12 Tf 0 g".to_vec(), StringFormat::Literal),
            "DR" => Object::Dictionary(dr_dict),
        }));

        // --- Catalog ---
        let catalog_id = doc.add_object(Object::Dictionary(dictionary! {
            "Type" => Object::Name(b"Catalog".to_vec()),
            "Pages" => Object::Reference(pages_id),
            "AcroForm" => Object::Reference(acroform_id),
        }));

        doc.trailer.set("Root", Object::Reference(catalog_id));

        let ids = TestPdfIds {
            catalog_id,
            acroform_id,
            page_id,
            pages_id,
            name_field_id,
            agree_field_id,
            state_field_id,
            account_parent_id,
            account_number_id,
            orphan_field_id,
        };

        (doc, ids)
    }

    // ─── pdf_string_to_rust ───

    #[test]
    fn pdf_string_to_rust_utf8() {
        let obj = Object::String(b"Hello World".to_vec(), StringFormat::Literal);
        assert_eq!(pdf_string_to_rust(&obj), Ok("Hello World".to_string()));
    }

    #[test]
    fn pdf_string_to_rust_utf16be() {
        // UTF-16BE BOM (FE FF) + "Hi" (0x0048, 0x0069)
        let bytes = vec![0xFE, 0xFF, 0x00, 0x48, 0x00, 0x69];
        let obj = Object::String(bytes, StringFormat::Literal);
        assert_eq!(pdf_string_to_rust(&obj), Ok("Hi".to_string()));
    }

    #[test]
    fn pdf_string_to_rust_name_object() {
        let obj = Object::Name(b"MyField".to_vec());
        assert_eq!(pdf_string_to_rust(&obj), Ok("MyField".to_string()));
    }

    #[test]
    fn pdf_string_to_rust_non_string() {
        let obj = Object::Integer(42);
        assert_eq!(pdf_string_to_rust(&obj), Err(()));
    }

    // ─── resolve_object ───

    #[test]
    fn resolve_object_direct() {
        let doc = Document::with_version("1.5");
        let obj = Object::Integer(99);
        let result = resolve_object(&doc, &obj);
        assert_eq!(*result, Object::Integer(99));
    }

    #[test]
    fn resolve_object_reference() {
        let mut doc = Document::with_version("1.5");
        let target = Object::Integer(42);
        let id = doc.add_object(target);
        let ref_obj = Object::Reference(id);
        let result = resolve_object(&doc, &ref_obj);
        assert_eq!(*result, Object::Integer(42));
    }

    // ─── resolve_dict ───

    #[test]
    fn resolve_dict_direct_dictionary() {
        let doc = Document::with_version("1.5");
        let dict = dictionary! { "Key" => Object::Boolean(true) };
        let obj = Object::Dictionary(dict);
        let result = resolve_dict(&doc, &obj);
        assert!(result.is_some());
        assert_eq!(result.unwrap().get(b"Key").unwrap(), &Object::Boolean(true));
    }

    #[test]
    fn resolve_dict_reference_to_dict() {
        let mut doc = Document::with_version("1.5");
        let dict = dictionary! { "Foo" => Object::Integer(7) };
        let id = doc.add_object(Object::Dictionary(dict));
        let ref_obj = Object::Reference(id);
        let result = resolve_dict(&doc, &ref_obj);
        assert!(result.is_some());
        assert_eq!(result.unwrap().get(b"Foo").unwrap(), &Object::Integer(7));
    }

    #[test]
    fn resolve_dict_non_dict() {
        let doc = Document::with_version("1.5");
        let obj = Object::Integer(5);
        assert!(resolve_dict(&doc, &obj).is_none());
    }

    // ─── get_field_name ───

    #[test]
    fn get_field_name_simple() {
        let (doc, ids) = create_test_pdf();
        let name = get_field_name(&doc, ids.name_field_id);
        assert_eq!(name, Some("Name".to_string()));
    }

    #[test]
    fn get_field_name_hierarchical() {
        let (doc, ids) = create_test_pdf();
        let name = get_field_name(&doc, ids.account_number_id);
        assert_eq!(name, Some("Account.Number".to_string()));
    }

    #[test]
    fn get_field_name_no_t_key() {
        let mut doc = Document::with_version("1.5");
        let id = doc.add_object(Object::Dictionary(dictionary! {
            "FT" => Object::Name(b"Tx".to_vec()),
        }));
        assert_eq!(get_field_name(&doc, id), None);
    }

    // ─── get_field_type ───

    #[test]
    fn get_field_type_direct() {
        let (doc, ids) = create_test_pdf();
        assert_eq!(
            get_field_type(&doc, ids.name_field_id),
            Some("Tx".to_string())
        );
        assert_eq!(
            get_field_type(&doc, ids.agree_field_id),
            Some("Btn".to_string())
        );
        assert_eq!(
            get_field_type(&doc, ids.state_field_id),
            Some("Ch".to_string())
        );
    }

    #[test]
    fn get_field_type_inherited() {
        // Create a child that has no FT but whose parent does
        let mut doc = Document::with_version("1.5");
        let parent_id = doc.add_object(Object::Dictionary(dictionary! {
            "FT" => Object::Name(b"Tx".to_vec()),
            "T" => Object::String(b"Parent".to_vec(), StringFormat::Literal),
        }));
        let child_id = doc.add_object(Object::Dictionary(dictionary! {
            "T" => Object::String(b"Child".to_vec(), StringFormat::Literal),
            "Parent" => Object::Reference(parent_id),
        }));
        assert_eq!(get_field_type(&doc, child_id), Some("Tx".to_string()));
    }

    #[test]
    fn get_field_type_none() {
        let mut doc = Document::with_version("1.5");
        let id = doc.add_object(Object::Dictionary(dictionary! {
            "T" => Object::String(b"NoType".to_vec(), StringFormat::Literal),
        }));
        assert_eq!(get_field_type(&doc, id), None);
    }

    // ─── set_field_value ───

    #[test]
    fn set_field_value_text() {
        let (mut doc, ids) = create_test_pdf();
        set_field_value(&mut doc, ids.name_field_id, "Name", "Alice");
        let obj = doc.get_object(ids.name_field_id).unwrap();
        let dict = obj.as_dict().unwrap();
        assert_eq!(
            dict.get(b"V").unwrap(),
            &Object::String(b"Alice".to_vec(), StringFormat::Literal)
        );
    }

    #[test]
    fn set_field_value_button() {
        let (mut doc, ids) = create_test_pdf();
        set_field_value(&mut doc, ids.agree_field_id, "Agree", "Yes");
        let obj = doc.get_object(ids.agree_field_id).unwrap();
        let dict = obj.as_dict().unwrap();
        assert_eq!(dict.get(b"V").unwrap(), &Object::Name(b"Yes".to_vec()));
        assert_eq!(dict.get(b"AS").unwrap(), &Object::Name(b"Yes".to_vec()));
    }

    #[test]
    fn set_field_value_choice() {
        let (mut doc, ids) = create_test_pdf();
        set_field_value(&mut doc, ids.state_field_id, "State", "NY");
        let obj = doc.get_object(ids.state_field_id).unwrap();
        let dict = obj.as_dict().unwrap();
        assert_eq!(
            dict.get(b"V").unwrap(),
            &Object::String(b"NY".to_vec(), StringFormat::Literal)
        );
    }

    // ─── set_need_appearances ───

    #[test]
    fn set_need_appearances_sets_flag() {
        let (mut doc, ids) = create_test_pdf();
        set_need_appearances(&mut doc);
        let acroform = doc.get_object(ids.acroform_id).unwrap();
        let dict = acroform.as_dict().unwrap();
        assert_eq!(
            dict.get(b"NeedAppearances").unwrap(),
            &Object::Boolean(true)
        );
    }

    // ─── collect_field_ids ───

    #[test]
    fn collect_field_ids_finds_all() {
        let (doc, _ids) = create_test_pdf();
        let field_ids = collect_field_ids(&doc);
        // Should find: Name, Agree, State, Account (parent), Account.Number (child via Kids)
        assert_eq!(field_ids.len(), 5);
    }

    // ─── collect_page_annotation_ids ───

    #[test]
    fn collect_page_annotation_ids_finds_annots() {
        let (doc, _ids) = create_test_pdf();
        let annot_ids = collect_page_annotation_ids(&doc);
        // Page Annots has: Name, Agree, State, OrphanField
        assert_eq!(annot_ids.len(), 4);
    }

    // ─── fill_pdf ───

    #[test]
    fn fill_pdf_fills_matching() {
        let (mut doc, ids) = create_test_pdf();
        let mut data = HashMap::new();
        data.insert("Name".to_string(), "Alice".to_string());
        data.insert("Agree".to_string(), "Yes".to_string());
        data.insert("Account.Number".to_string(), "99999".to_string());

        let filled = fill_pdf(&mut doc, &data);
        assert!(filled.contains("Name"));
        assert!(filled.contains("Agree"));
        assert!(filled.contains("Account.Number"));

        // Verify values were actually set
        let name_obj = doc.get_object(ids.name_field_id).unwrap();
        let name_dict = name_obj.as_dict().unwrap();
        assert_eq!(
            name_dict.get(b"V").unwrap(),
            &Object::String(b"Alice".to_vec(), StringFormat::Literal)
        );
    }

    #[test]
    fn fill_pdf_fills_orphan() {
        let (mut doc, ids) = create_test_pdf();
        let mut data = HashMap::new();
        data.insert("OrphanField".to_string(), "orphan_value".to_string());

        let filled = fill_pdf(&mut doc, &data);
        assert!(filled.contains("OrphanField"));

        let orphan_obj = doc.get_object(ids.orphan_field_id).unwrap();
        let orphan_dict = orphan_obj.as_dict().unwrap();
        assert_eq!(
            orphan_dict.get(b"V").unwrap(),
            &Object::String(b"orphan_value".to_vec(), StringFormat::Literal)
        );
    }

    #[test]
    fn fill_pdf_skips_empty() {
        let (mut doc, ids) = create_test_pdf();
        let mut data = HashMap::new();
        data.insert("Name".to_string(), "".to_string());

        let filled = fill_pdf(&mut doc, &data);
        assert!(!filled.contains("Name"));

        // V should not be set
        let name_obj = doc.get_object(ids.name_field_id).unwrap();
        let name_dict = name_obj.as_dict().unwrap();
        assert!(name_dict.get(b"V").is_err());
    }

    // ─── list_field_names ───

    #[test]
    fn list_field_names_sorted_deduped() {
        let (doc, _ids) = create_test_pdf();
        let names = list_field_names(&doc);
        // Expected: Account, Account.Number, Agree, Name, OrphanField, State
        // Note: Account parent has a T key so it shows up, plus child Account.Number
        assert_eq!(
            names,
            vec![
                "Account",
                "Account.Number",
                "Agree",
                "Name",
                "OrphanField",
                "State",
            ]
        );
    }

    // ─── pdf_escape / wrap_text ───

    #[test]
    fn pdf_escape_special_chars() {
        assert_eq!(pdf_escape("hello"), "hello");
        assert_eq!(pdf_escape("a(b)c"), "a\\(b\\)c");
        assert_eq!(pdf_escape("back\\slash"), "back\\\\slash");
    }

    #[test]
    fn wrap_text_wraps_at_word_boundaries() {
        // width 60pt, size 10 → ~12 chars/line
        let lines = wrap_text("the quick brown fox jumps", 60.0, 10.0);
        assert!(lines.len() >= 2, "should wrap: {lines:?}");
        // explicit newline forces a break
        assert_eq!(
            wrap_text("a\nb", 200.0, 10.0),
            vec!["a".to_string(), "b".to_string()]
        );
        // a word longer than the line is kept intact (never split) — it overflows/clips
        assert_eq!(
            wrap_text("supercalifragilistic", 30.0, 10.0),
            vec!["supercalifragilistic".to_string()]
        );
    }

    // ─── parse_font_size_from_da ───

    #[test]
    fn parse_font_size() {
        assert_eq!(parse_font_size_from_da("/Helv 12 Tf 0 g"), Some(12.0));
    }

    #[test]
    fn parse_font_size_none_without_tf() {
        assert_eq!(parse_font_size_from_da("0 g"), None);
    }

    // ─── generate_appearance_stream ───

    #[test]
    fn generate_appearance_stream_sets_ap() {
        let (mut doc, ids) = create_test_pdf();
        generate_appearance_stream(&mut doc, ids.name_field_id, "John");

        let obj = doc.get_object(ids.name_field_id).unwrap();
        let dict = obj.as_dict().unwrap();
        let ap = dict.get(b"AP").unwrap().as_dict().unwrap();
        let n_ref = ap.get(b"N").unwrap().as_reference().unwrap();

        // Verify the referenced object is a stream
        let stream_obj = doc.get_object(n_ref).unwrap();
        assert!(stream_obj.as_stream().is_ok());
    }

    #[test]
    fn generate_appearance_stream_skips_no_rect() {
        let mut doc = Document::with_version("1.5");
        // Field without /Rect
        let field_id = doc.add_object(Object::Dictionary(dictionary! {
            "FT" => Object::Name(b"Tx".to_vec()),
            "T" => Object::String(b"NoRect".to_vec(), StringFormat::Literal),
            "DA" => Object::String(b"/Helv 12 Tf 0 g".to_vec(), StringFormat::Literal),
        }));

        generate_appearance_stream(&mut doc, field_id, "test");

        // Should not have /AP since there's no /Rect
        let obj = doc.get_object(field_id).unwrap();
        let dict = obj.as_dict().unwrap();
        assert!(dict.get(b"AP").is_err());
    }

    #[test]
    fn fill_pdf_generates_appearances() {
        let (mut doc, ids) = create_test_pdf();
        let mut data = HashMap::new();
        data.insert("Name".to_string(), "Alice".to_string());
        data.insert("State".to_string(), "NY".to_string());

        fill_pdf(&mut doc, &data);

        // Text field "Name" should have /AP
        let name_obj = doc.get_object(ids.name_field_id).unwrap();
        let name_dict = name_obj.as_dict().unwrap();
        assert!(name_dict.get(b"AP").is_ok(), "Name field should have /AP");

        // Choice field "State" should have /AP
        let state_obj = doc.get_object(ids.state_field_id).unwrap();
        let state_dict = state_obj.as_dict().unwrap();
        assert!(state_dict.get(b"AP").is_ok(), "State field should have /AP");
    }

    #[test]
    fn fill_pdf_skips_ap_for_buttons() {
        let (mut doc, ids) = create_test_pdf();
        let mut data = HashMap::new();
        data.insert("Agree".to_string(), "Yes".to_string());

        fill_pdf(&mut doc, &data);

        // Button field should NOT get a generated /AP (they use pre-built appearances)
        let btn_obj = doc.get_object(ids.agree_field_id).unwrap();
        let btn_dict = btn_obj.as_dict().unwrap();
        assert!(
            btn_dict.get(b"AP").is_err(),
            "Button field should not have generated /AP"
        );
    }

    #[test]
    fn fill_pdf_keeps_need_appearances() {
        let (mut doc, ids) = create_test_pdf();

        let mut data = HashMap::new();
        data.insert("Name".to_string(), "Test".to_string());
        fill_pdf(&mut doc, &data);

        // NeedAppearances must stay true so Adobe's form engine rebuilds malformed
        // template's fields (including checkbox state) on open.
        let acroform = doc.get_object(ids.acroform_id).unwrap();
        let dict = acroform.as_dict().unwrap();
        assert_eq!(
            dict.get(b"NeedAppearances").unwrap(),
            &Object::Boolean(true)
        );
    }
}
