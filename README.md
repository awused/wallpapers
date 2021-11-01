Wallpapers
==========

A tool for managing and shuffling a large number of wallpapers across multiple monitors, using waifu2x for upscaling. Runs on Windows 8 and up and Linux (X11, non-gnome).

# Requirements

* Windows 8+.
    * Newer APIs added after Windows 7 remove the need for hacky workarounds.
* Linux:
    * Currently only works on X11.
    * Feh is required.
    * Does not work on Gnome, works only where feh works.

Upscaling has additional default requirements, but can be configured to use others:

* [waifu2x-ncnn-vulkan](https://github.com/nihui/waifu2x-ncnn-vulkan) When installing waifu2x, make sure that the [models](https://github.com/nihui/waifu2x-ncnn-vulkan/tree/master/models) directory is present (copied or symlinked) in the same directory as the executable.
* [PyGObject](https://pygobject.readthedocs.io/) is also preferred by the default upscaler.
    * [ImageMagick 6 or 7](https://imagemagick.org/script/download.php) will be used as a fallback if PyGobject is not available.

Alternative upscalers can be configured in place of waifu2x-ncnn-vulkan, see [aw-upscale](https://github.com/awused/aw-upscale).

If you have trouble getting upscaling to work, make sure that waifu2x-ncnn-vulkan is on your PATH. The directory containing the waifu2x-ncnn-vulkan binary should also contain the [models-cunet](https://github.com/nihui/waifu2x-ncnn-vulkan/tree/master/models/models-cunet) directory.

### Limitations

* Limited to one X server

# Usage

`cargo install --git https://github.com/awused/wallpapers --locked`

Install with `--features windows-quiet` on Windows to avoid spawning a visible console Window. Note that this will also disable stdout.

Fill in wallpapers.toml and copy it to your choice of /usr/local/etc/wallpapers.toml, /usr/etc/wallpapers.toml, or $HOME/.wallpapers.toml. On Windows it's easiest to just drop it into the same directory as the executable.

Run `wallpapers sync` to prepopulate the cache for your current set of wallpapers. This can be a very time consuming operation and stresses both your CPU and GPU. It can take hours to run for hundreds or thousands of images. The cache can take considerably more space than your original wallpapers, especially with high resolution monitors, so make sure there is sufficient disk space.

`wallpapers help` will display additional usage information.

I've included some scripts and registry files for context menu entries that I find useful under [windows](windows) and [linux](linux). They must be edited before use.

## Commands
### Random

`wallpapers random`

The random command will set one random wallpaper randomly on each monitor. It favours less recently selected wallpapers (See [go-strpick](https://github.com/awused/go-strpick)) and will not select the same wallpaper for multiple monitors at the same time when there are enough wallpapers to avoid it.

If one of the selected wallpapers hasn't been cached it will perform the same upscaling and caching as sync. If you're running this as part of a periodic task or cron job this can interrupt whatever you are doing by stressing your GPU, so it's recommended to run sync manually so you can control the timing.


### Preview

`wallpapers preview wallpaper.jpg`

The preview command is used to show you what a wallpaper will look like after processing. Using different flags it's possible to set all the same image manipulation values that are available in the config file, allowing you to dial in the specific settings you want. See `wallpapers preview -h` for more details.


### Sync

`wallpapers sync`

The sync will prepopulate the cache for the currently selected set of monitors and remove any invalid cache entries. It will also remove entries from the usage database, which is not done as part of normal operation. As stated above this can be a time consuming and CPU/GPU intensive process, but once the initial sync completes subsequent runs will only touch modified files.

One thing sync does not do by default is remove cached images for monitors that are no longer attached. If you have a laptop that you connect periodically to a 4K monitor, those 4K images will be untouched. You'll need to specify `--clean_monitors` to delete them.


## Interactive

`wallpapers interactive wallpaper.jpg`

Interactively preview a wallpaper on all your monitors, reusing processed files so you can quickly dial in your settings. Using `vertical` and `horizontal` offsets are more efficient than cropping for this as changes will not need to be run through waifu2x. Use the print command to print out a snippet of TOML that can be copied into the configuration file.

On Windows you'll want probably want to build a separate executable without hiding the console. This can be done by not specifying `--features windows-quiet`. The other option is to use a wrapper program when calling wallpapers.exe from a scheduled task.


# Image Manipulation

One of the biggest annoyances is dealing with different aspect ratios between your monitors, making some images look bad on some monitors, or images that just don't work well as wallpapers because they're too wide or too tall. Using different settings it's possible to crop, letterbox, or offset your wallpapers differently for all of your different aspect ratios.

Settings are all set per aspect ratio. So all 16:9 monitors, regardless of their actual resolution, will use the same settings for the same wallpapers. The configuration format is also explained, with examples, in .properties.toml, which you can place in the root of your originals_directory.


## Cropping/Letterboxing
The Top, Bottom, Left, and Right values are integers that control how many pixels are cropped from the original image. Negative integers result in padding.

Background is the colour used when padding images. It defaults to black, but can be "black", "white", or an RRGGBB hex string of the form "a1b2c3".


## Offsets
Using Vertical or Horizontal values, as decimal percentages can give you fine-grained control over exactly how much you translate an image up/down or right/left, which is useful when the image is taller or wider than your monitor.

It only ever makes sense to specify one of them at a time and any offset can instead be done using a crop. Offsets are more efficient than cropping in interactive mode, while still handling the majority of problematic wallpapers.

## Denoising
There isn't a one-size-fits-all setting for denoising, so it is configurable. The default level is 1 as this has minimal effects on images without noise and is sufficient for cleaning up most images with mild artifacts.

