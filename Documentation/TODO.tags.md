If you are working directly with the FFmpeg C libraries (libavformat, libavcodec, libavutil), the reason the ffprobe executable isn't showing that data is because the standard mov demuxer doesn't automatically map udta/name boxes to the metadata dictionary of an AVStream.

To access this, you have to dig into the "side data" or the raw packet/stream metadata using specific flags during the dictionary allocation.

1. Enable "Export All" in Code

In the ffprobe source code, the -export_all flag sets a specific entry in the AVFormatContext. In your C/C++ code, you must set this before calling avformat_open_input:

```
AVDictionary *options = NULL;
// This is the equivalent of -export_all 1
av_dict_set(&options, "export_all", "1", 0);

if (avformat_open_input(&format_ctx, filename, NULL, &options) < 0) {
    // Handle error
}
```

2. Accessing the Metadata

Once the file is opened and avformat_find_stream_info is called, the metadata for each stream is stored in format_ctx->streams[i]->metadata.

If export_all is working, you should be able to iterate through the tags. Note that FFmpeg often prefixes these "hidden" boxes. Look for keys like name or com.apple.quicktime.name.

```
AVDictionaryEntry *tag = NULL;
while ((tag = av_dict_get(stream->metadata, "", tag, AV_DICT_IGNORE_SUFFIX))) {
    printf("Track %d Tag: %s = %s\n", stream_index, tag->key, tag->value);
}
```

3. The "Deep Dive" (Manual Box Parsing)

If the export_all flag still doesn't surface the udta box (which happens if the box isn't in the demuxer's "recognized" list), you have to look at the AVStream side data.

FFmpeg sometimes stores unparsed or "unknown" hardware-specific metadata as side data. You can check for AV_PKT_DATA_STRINGS_METADATA or similar types, though this is rare for udta.


