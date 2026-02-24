#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send<T: Send>() {}

    #[test]
    fn test_input_is_send() {
        assert_send::<ffmpeg_next::format::context::Input>();
    }
}
