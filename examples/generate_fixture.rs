//! Generates `tests/fixtures/test_form.pdf` — a simple PDF with fillable form fields.
//!
//! Run with: `cargo run --example generate_fixture`

use lopdf::dictionary;
use lopdf::{Document, Object, StringFormat};

fn main() {
    let mut doc = Document::with_version("1.5");

    // --- Form fields ---

    // Text field: Name
    let name_field = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => Object::Name(b"Annot".to_vec()),
        "Subtype" => Object::Name(b"Widget".to_vec()),
        "FT" => Object::Name(b"Tx".to_vec()),
        "T" => Object::String(b"Name".to_vec(), StringFormat::Literal),
        "Rect" => Object::Array(vec![
            Object::Real(50.0), Object::Real(700.0),
            Object::Real(250.0), Object::Real(720.0),
        ]),
    }));

    // Text field: Email
    let email_field = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => Object::Name(b"Annot".to_vec()),
        "Subtype" => Object::Name(b"Widget".to_vec()),
        "FT" => Object::Name(b"Tx".to_vec()),
        "T" => Object::String(b"Email".to_vec(), StringFormat::Literal),
        "Rect" => Object::Array(vec![
            Object::Real(50.0), Object::Real(660.0),
            Object::Real(250.0), Object::Real(680.0),
        ]),
    }));

    // Checkbox: Agree (FT=Btn)
    let agree_field = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => Object::Name(b"Annot".to_vec()),
        "Subtype" => Object::Name(b"Widget".to_vec()),
        "FT" => Object::Name(b"Btn".to_vec()),
        "T" => Object::String(b"Agree".to_vec(), StringFormat::Literal),
        "Rect" => Object::Array(vec![
            Object::Real(50.0), Object::Real(620.0),
            Object::Real(70.0), Object::Real(640.0),
        ]),
    }));

    // Dropdown: State (FT=Ch) with options
    let state_field = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => Object::Name(b"Annot".to_vec()),
        "Subtype" => Object::Name(b"Widget".to_vec()),
        "FT" => Object::Name(b"Ch".to_vec()),
        "T" => Object::String(b"State".to_vec(), StringFormat::Literal),
        "DV" => Object::String(b"CA".to_vec(), StringFormat::Literal),
        "V" => Object::String(b"CA".to_vec(), StringFormat::Literal),
        "Opt" => Object::Array(vec![
            Object::String(b"CA".to_vec(), StringFormat::Literal),
            Object::String(b"NY".to_vec(), StringFormat::Literal),
            Object::String(b"TX".to_vec(), StringFormat::Literal),
        ]),
        "Rect" => Object::Array(vec![
            Object::Real(50.0), Object::Real(580.0),
            Object::Real(250.0), Object::Real(600.0),
        ]),
    }));

    // Hierarchical: Account parent → Number child
    let account_parent = doc.add_object(Object::Dictionary(dictionary! {
        "T" => Object::String(b"Account".to_vec(), StringFormat::Literal),
    }));

    let account_number = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => Object::Name(b"Annot".to_vec()),
        "Subtype" => Object::Name(b"Widget".to_vec()),
        "FT" => Object::Name(b"Tx".to_vec()),
        "T" => Object::String(b"Number".to_vec(), StringFormat::Literal),
        "Parent" => Object::Reference(account_parent),
        "Rect" => Object::Array(vec![
            Object::Real(50.0), Object::Real(540.0),
            Object::Real(250.0), Object::Real(560.0),
        ]),
    }));

    // Update parent with Kids
    doc.set_object(
        account_parent,
        Object::Dictionary(dictionary! {
            "T" => Object::String(b"Account".to_vec(), StringFormat::Literal),
            "Kids" => Object::Array(vec![Object::Reference(account_number)]),
        }),
    );

    // --- Page ---
    let pages_id = doc.new_object_id();

    let page = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => Object::Name(b"Page".to_vec()),
        "Parent" => Object::Reference(pages_id),
        "MediaBox" => Object::Array(vec![
            Object::Integer(0), Object::Integer(0),
            Object::Integer(612), Object::Integer(792),
        ]),
        "Annots" => Object::Array(vec![
            Object::Reference(name_field),
            Object::Reference(email_field),
            Object::Reference(agree_field),
            Object::Reference(state_field),
            Object::Reference(account_number),
        ]),
    }));

    doc.set_object(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => Object::Name(b"Pages".to_vec()),
            "Count" => Object::Integer(1),
            "Kids" => Object::Array(vec![Object::Reference(page)]),
        }),
    );

    // --- AcroForm ---
    let acroform = doc.add_object(Object::Dictionary(dictionary! {
        "Fields" => Object::Array(vec![
            Object::Reference(name_field),
            Object::Reference(email_field),
            Object::Reference(agree_field),
            Object::Reference(state_field),
            Object::Reference(account_parent),
        ]),
        "NeedAppearances" => Object::Boolean(true),
    }));

    // --- Catalog ---
    let catalog = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => Object::Name(b"Catalog".to_vec()),
        "Pages" => Object::Reference(pages_id),
        "AcroForm" => Object::Reference(acroform),
    }));

    doc.trailer.set("Root", Object::Reference(catalog));

    let out_path = "tests/fixtures/test_form.pdf";
    doc.save(out_path).expect("failed to save test_form.pdf");
    println!("Generated {out_path}");
}
