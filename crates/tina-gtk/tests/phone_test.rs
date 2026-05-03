#[test]
fn test_phone_formatting() {
    let num_with_plus = "+556196862399";
    let parsed = phonenumber::parse(None, num_with_plus);
    if let Ok(phone) = parsed {
        println!("Valid: {}", phone.is_valid());
        println!(
            "International: {}",
            phone.format().mode(phonenumber::Mode::International)
        );
    }

    let landline = "+551140044004";
    let parsed2 = phonenumber::parse(None, landline);
    if let Ok(phone) = parsed2 {
        println!("Landline valid: {}", phone.is_valid());
        println!(
            "Landline International: {}",
            phone.format().mode(phonenumber::Mode::International)
        );
    }
}
