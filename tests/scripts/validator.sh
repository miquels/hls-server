#! /bin/sh

BASE=http://localhost:3000
VIDEO=tests/assets/video.mp4

case "$1" in
  "") ;;
  -*)
    echo "unknown option $1" >&2
    exit 1
    ;;
  *)
    VIDEO="$1"
    ;;
esac

if [ ! -f "$VIDEO" ]; then
  echo "$VIDEO: not found" >&2
  exit 1
fi
VIDEO=$(realpath "$VIDEO").as.m3u8

mediastreamvalidator $BASE$VIDEO
rm -f validation_data.json

