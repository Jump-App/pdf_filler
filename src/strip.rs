use lopdf::Document;
use std::collections::HashSet;

use crate::fill;

/// Strip /DV and /V from unfilled Choice (dropdown) fields.
/// This prevents PDF viewers from showing template default values on empty dropdowns.
pub fn strip_unfilled_dropdowns(doc: &mut Document, filled_keys: &HashSet<String>) {
    // Collect field IDs using the same AcroForm traversal as fill_pdf,
    // plus page annotations for orphan widgets.
    let mut field_ids = fill::collect_field_ids(doc);
    field_ids.extend(fill::collect_page_annotation_ids(doc));

    for field_id in field_ids {
        let is_choice = fill::get_field_type(doc, field_id)
            .map(|ft| ft == "Ch")
            .unwrap_or(false);

        let field_name = fill::get_field_name(doc, field_id).unwrap_or_default();

        if is_choice && !field_name.is_empty() && !filled_keys.contains(&field_name) {
            if let Ok(obj) = doc.get_object_mut(field_id) {
                if let Ok(dict) = obj.as_dict_mut() {
                    dict.remove(b"DV");
                    dict.remove(b"V");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fill::tests::create_test_pdf;
    use lopdf::Object;

    #[test]
    fn strip_removes_dv_v_from_unfilled_choice() {
        let (mut doc, ids) = create_test_pdf();
        let filled = HashSet::new(); // nothing filled

        strip_unfilled_dropdowns(&mut doc, &filled);

        let obj = doc.get_object(ids.state_field_id).unwrap();
        let dict = obj.as_dict().unwrap();
        assert!(dict.get(b"DV").is_err(), "DV should be removed");
        assert!(dict.get(b"V").is_err(), "V should be removed");
    }

    #[test]
    fn strip_preserves_filled_choice() {
        let (mut doc, ids) = create_test_pdf();
        let mut filled = HashSet::new();
        filled.insert("State".to_string());

        strip_unfilled_dropdowns(&mut doc, &filled);

        let obj = doc.get_object(ids.state_field_id).unwrap();
        let dict = obj.as_dict().unwrap();
        assert!(dict.get(b"DV").is_ok(), "DV should be preserved");
        assert!(dict.get(b"V").is_ok(), "V should be preserved");
    }

    #[test]
    fn strip_does_not_touch_non_choice() {
        let (mut doc, ids) = create_test_pdf();

        // First set a value on the text field
        fill::set_field_value(&mut doc, ids.name_field_id, "Name", "Alice");

        let filled = HashSet::new(); // nothing in filled set
        strip_unfilled_dropdowns(&mut doc, &filled);

        // Text field's V should still be there
        let obj = doc.get_object(ids.name_field_id).unwrap();
        let dict = obj.as_dict().unwrap();
        assert_eq!(
            dict.get(b"V").unwrap(),
            &Object::String(b"Alice".to_vec(), lopdf::StringFormat::Literal)
        );
    }
}
