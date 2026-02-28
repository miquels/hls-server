#[cfg(test)]
mod tests {

    fn assert_send<T: Send>() {}

    #[test]
    fn test_input_is_send() {
        assert_send::<ffmpeg_next::format::context::Input>();
    }
}
