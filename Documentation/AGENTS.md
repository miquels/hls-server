# hls-server project
 
Written in Rust.

## Dev environment tips
- For dependencies, only use battle-tested crates with at least 10000 downloads from crates.io
- Use 'cargo check' to check the code for errors and warnings
 
## Testing instructions
- Ask the user to start / stop / restart the server.
- See the @README.md for API endpoints - under "### Create a Stream".
- You can use the file 'testvideos/bun33s.mp4' as a test video (short big bucks bunny testvideo)
- Use the "master_playlist" you get back in the reply from the registration endpoint for further tests
- You can use curl to get the master playlist, then the video / audio playlists, init segments, media segments
- You can use the tool 'mediastreamvalidator' to validate the master playlist

