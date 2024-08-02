pub fn elide(str: &String) -> String {
    let len = str.len();
    if len <= 64 {
        format!("{:?}", str)
    } else {
        format!(
            "{:?}… (len {})",
            String::from_utf8_lossy(&str.as_bytes()[..63]),
            len
        )
    }
}
