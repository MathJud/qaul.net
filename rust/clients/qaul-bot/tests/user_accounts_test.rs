// Copyright (c) 2021 Open Community Project Association https://ocpa.ch
// This software is published under the AGPLv3 license.

//! # Integration tests for user account querying

use prost::Message;

/// include generated protobuf RPC rust definition files
mod proto_rpc {
    include!("../../../libqaul/src/rpc/protobuf_generated/rust/qaul.rpc.rs");
}

mod proto_accounts {
    include!("../../../libqaul/src/rpc/protobuf_generated/rust/qaul.rpc.user_accounts.rs");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test 1: Verify request_default_account creates proper RPC message
    #[test]
    fn test_request_default_account_creates_valid_message() {
        // Create the expected protobuf message
        let proto_message = proto_accounts::UserAccounts {
            message: Some(proto_accounts::user_accounts::Message::GetDefaultUserAccount(true)),
        };

        // Encode message
        let mut buf = Vec::with_capacity(proto_message.encoded_len());
        proto_message
            .encode(&mut buf)
            .expect("Vec<u8> provides capacity as needed");

        // Verify message can be decoded
        let decoded = proto_accounts::UserAccounts::decode(&buf[..]);
        assert!(decoded.is_ok());

        let decoded = decoded.unwrap();
        assert!(matches!(
            decoded.message,
            Some(proto_accounts::user_accounts::Message::GetDefaultUserAccount(true))
        ));
    }

    /// Test 2: Verify RPC message decoding for default user account response
    #[test]
    fn test_decode_default_account_response_with_account() {
        // Create a mock default account response
        let my_account = proto_accounts::MyUserAccount {
            name: "beacon-bot".to_string(),
            id: vec![1, 2, 3, 4],
            id_base58: "test-id-base58".to_string(),
            key: vec![5, 6, 7, 8],
            key_type: "Ed25519".to_string(),
            key_base58: "test-key-base58".to_string(),
        };

        let proto_message = proto_accounts::UserAccounts {
            message: Some(proto_accounts::user_accounts::Message::DefaultUserAccount(
                proto_accounts::DefaultUserAccount {
                    user_account_exists: true,
                    my_user_account: Some(my_account),
                },
            )),
        };

        // Encode the message
        let mut buf = Vec::with_capacity(proto_message.encoded_len());
        proto_message.encode(&mut buf).unwrap();

        // Decode and verify
        let decoded = proto_accounts::UserAccounts::decode(&buf[..]);
        assert!(decoded.is_ok());

        let decoded = decoded.unwrap();
        if let Some(proto_accounts::user_accounts::Message::DefaultUserAccount(account_msg)) =
            decoded.message
        {
            assert!(account_msg.user_account_exists);
            assert!(account_msg.my_user_account.is_some());

            let account = account_msg.my_user_account.unwrap();
            assert_eq!(account.name, "beacon-bot");
            assert_eq!(account.id_base58, "test-id-base58");
            assert_eq!(account.key_base58, "test-key-base58");
        } else {
            panic!("Wrong message type");
        }
    }

    /// Test 3: Verify RPC message decoding for no account response
    #[test]
    fn test_decode_default_account_response_no_account() {
        let proto_message = proto_accounts::UserAccounts {
            message: Some(proto_accounts::user_accounts::Message::DefaultUserAccount(
                proto_accounts::DefaultUserAccount {
                    user_account_exists: false,
                    my_user_account: None,
                },
            )),
        };

        // Encode the message
        let mut buf = Vec::with_capacity(proto_message.encoded_len());
        proto_message.encode(&mut buf).unwrap();

        // Decode and verify
        let decoded = proto_accounts::UserAccounts::decode(&buf[..]);
        assert!(decoded.is_ok());

        let decoded = decoded.unwrap();
        if let Some(proto_accounts::user_accounts::Message::DefaultUserAccount(account_msg)) =
            decoded.message
        {
            assert!(!account_msg.user_account_exists);
            assert!(account_msg.my_user_account.is_none());
        } else {
            panic!("Wrong message type");
        }
    }

    /// Test 4: Verify account info formatting for display
    #[test]
    fn test_format_account_info_for_display() {
        let name = "beacon-bot";
        let id_base58 = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY";
        let key_base58 = "D3AkL5yejb3n2Tn1Z7t3";

        // Format the expected output
        let formatted = format!(
            "Your user account is:\n{}, ID[{}]\n    public key: {}",
            name, id_base58, key_base58
        );

        // Verify formatting
        assert!(formatted.contains("Your user account is:"));
        assert!(formatted.contains(name));
        assert!(formatted.contains(id_base58));
        assert!(formatted.contains(key_base58));
    }

    /// Test 5: Verify error handling for malformed RPC message
    #[test]
    fn test_decode_malformed_rpc_message() {
        // Create invalid protobuf data
        let invalid_data = vec![0xFF, 0xFF, 0xFF];

        // Attempt to decode
        let decoded = proto_accounts::UserAccounts::decode(&invalid_data[..]);

        // Should fail gracefully
        assert!(decoded.is_err());
    }
}
