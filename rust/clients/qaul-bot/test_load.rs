use std::fs;

#[path = "src/state.rs"]
mod state;

fn main() {
    let contents = fs::read_to_string("/home/sojus/src/vibe/cashu/qaul.net/rust/clients/Tbot/qaul-bot.yml").unwrap();
    println!("File contents:\n{}\n", contents);
    
    match serde_yaml::from_str::<state::BotState>(&contents) {
        Ok(state) => {
            println!("Successfully loaded state!");
            println!("nostr_keys present: {}", state.nostr_keys.is_some());
            if let Some(keys) = state.nostr_keys {
                println!("npub: {}", keys.npub);
                println!("private_key_hex length: {}", keys.private_key_hex.len());
            }
        }
        Err(e) => {
            println!("Failed to parse: {}", e);
        }
    }
}
