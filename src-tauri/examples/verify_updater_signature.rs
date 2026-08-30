use base64::{engine::general_purpose::STANDARD, Engine as _};
use minisign_verify::{PublicKey, Signature};
use std::{env, error::Error, fs};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        return Err(
            "usage: verify_updater_signature <archive> <signature> <public-key-b64-file>".into(),
        );
    }

    let archive = fs::read(&args[1])?;
    let encoded_signature = fs::read_to_string(&args[2])?;
    let encoded_public_key = fs::read_to_string(&args[3])?;

    let signature_text = String::from_utf8(STANDARD.decode(encoded_signature.trim())?)?;
    let public_key_text = String::from_utf8(STANDARD.decode(encoded_public_key.trim())?)?;
    let signature = Signature::decode(&signature_text)?;
    let public_key = PublicKey::decode(&public_key_text)?;

    public_key.verify(&archive, &signature, false)?;
    println!("Updater signature verified.");
    Ok(())
}
