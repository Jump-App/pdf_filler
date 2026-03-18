use lopdf::dictionary;
use lopdf::{Document, Object, ObjectId, StringFormat};

/// IDs returned by `create_test_pdf` so tests can reference specific objects.
#[allow(dead_code)]
pub struct TestPdfIds {
    pub name_field_id: ObjectId,
    pub agree_field_id: ObjectId,
    pub state_field_id: ObjectId,
    pub account_parent_id: ObjectId,
    pub account_number_id: ObjectId,
    pub orphan_field_id: ObjectId,
}

/// Build a minimal in-memory PDF with various field types for integration testing.
/// This duplicates the unit-test helper because integration tests cannot import
/// `#[cfg(test)]` modules from the binary crate.
pub fn create_test_pdf() -> (Document, TestPdfIds) {
    let mut doc = Document::with_version("1.5");

    // Text field "Name" (FT=Tx)
    let name_field_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => Object::Name(b"Annot".to_vec()),
        "Subtype" => Object::Name(b"Widget".to_vec()),
        "FT" => Object::Name(b"Tx".to_vec()),
        "T" => Object::String(b"Name".to_vec(), StringFormat::Literal),
    }));

    // Button field "Agree" (FT=Btn)
    let agree_field_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => Object::Name(b"Annot".to_vec()),
        "Subtype" => Object::Name(b"Widget".to_vec()),
        "FT" => Object::Name(b"Btn".to_vec()),
        "T" => Object::String(b"Agree".to_vec(), StringFormat::Literal),
    }));

    // Choice field "State" (FT=Ch) with DV and V defaults
    let state_field_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => Object::Name(b"Annot".to_vec()),
        "Subtype" => Object::Name(b"Widget".to_vec()),
        "FT" => Object::Name(b"Ch".to_vec()),
        "T" => Object::String(b"State".to_vec(), StringFormat::Literal),
        "DV" => Object::String(b"CA".to_vec(), StringFormat::Literal),
        "V" => Object::String(b"CA".to_vec(), StringFormat::Literal),
    }));

    // Hierarchical: parent "Account" → child "Number"
    let account_parent_id = doc.add_object(Object::Dictionary(dictionary! {
        "T" => Object::String(b"Account".to_vec(), StringFormat::Literal),
    }));

    let account_number_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => Object::Name(b"Annot".to_vec()),
        "Subtype" => Object::Name(b"Widget".to_vec()),
        "FT" => Object::Name(b"Tx".to_vec()),
        "T" => Object::String(b"Number".to_vec(), StringFormat::Literal),
        "Parent" => Object::Reference(account_parent_id),
    }));

    doc.set_object(
        account_parent_id,
        Object::Dictionary(dictionary! {
            "T" => Object::String(b"Account".to_vec(), StringFormat::Literal),
            "Kids" => Object::Array(vec![Object::Reference(account_number_id)]),
        }),
    );

    // Orphan annotation — on page but NOT in AcroForm Fields
    let orphan_field_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => Object::Name(b"Annot".to_vec()),
        "Subtype" => Object::Name(b"Widget".to_vec()),
        "FT" => Object::Name(b"Tx".to_vec()),
        "T" => Object::String(b"OrphanField".to_vec(), StringFormat::Literal),
    }));

    // Page
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

    doc.set_object(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => Object::Name(b"Pages".to_vec()),
            "Count" => Object::Integer(1),
            "Kids" => Object::Array(vec![Object::Reference(page_id)]),
        }),
    );

    // AcroForm
    let acroform_id = doc.add_object(Object::Dictionary(dictionary! {
        "Fields" => Object::Array(vec![
            Object::Reference(name_field_id),
            Object::Reference(agree_field_id),
            Object::Reference(state_field_id),
            Object::Reference(account_parent_id),
        ]),
    }));

    // Catalog
    let catalog_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => Object::Name(b"Catalog".to_vec()),
        "Pages" => Object::Reference(pages_id),
        "AcroForm" => Object::Reference(acroform_id),
    }));

    doc.trailer.set("Root", Object::Reference(catalog_id));

    let ids = TestPdfIds {
        name_field_id,
        agree_field_id,
        state_field_id,
        account_parent_id,
        account_number_id,
        orphan_field_id,
    };

    (doc, ids)
}
