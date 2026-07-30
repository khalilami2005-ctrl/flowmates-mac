const KEYCHAIN_SERVICE: &str = "eu.flowmates.mac";
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

pub fn set_secret(account: &str, value: &str) -> Result<(), String> {
    security_framework::passwords::set_generic_password(KEYCHAIN_SERVICE, account, value.as_bytes())
        .map_err(|e| format!("Could not save {account} in macOS Keychain: {e}"))
}

pub fn get_secret(account: &str) -> Result<Option<String>, String> {
    match security_framework::passwords::get_generic_password(KEYCHAIN_SERVICE, account) {
        Ok(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| format!("Keychain entry {account} is not valid UTF-8")),
        Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
        Err(error) => Err(format!(
            "Could not read {account} from macOS Keychain: {error}"
        )),
    }
}

pub fn delete_secret(account: &str) -> Result<(), String> {
    match security_framework::passwords::delete_generic_password(KEYCHAIN_SERVICE, account) {
        Ok(()) => Ok(()),
        Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
        Err(error) => Err(format!(
            "Could not remove {account} from macOS Keychain: {error}"
        )),
    }
}
