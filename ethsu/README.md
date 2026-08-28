# ethsu

`ethsu` is Ethereal's small SuperCall client. It is built as a static arm64
Android executable and packed into the boot ramdisk as `/eth/su`.

The numeric SuperCall magic, ioctl number, and command IDs are protocol ABI and
remain unchanged. Product paths and identifiers use Ethereal names only.
