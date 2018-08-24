Wallpapers
==========

A tool for managing and shuffling a large number of wallpapers across multiple monitors, using waifu2x for upscaling. Runs on Windows 8 and up.

<!-- TODO - And Linux, but with what limitations? -->

# Requirements

* ImageMagick
* Waifu2x
    * [waifu2x-caffe](https://github.com/lltcggie/waifu2x-caffe) on Windows <!-- TODO - Maybe just w2x-conv-cpp on both -->
    <!-- * [DeadSix27/waifu2x-converter-cpp](https://github.com/DeadSix27/waifu2x-converter-cpp) on Linux. Other forks of waifu2x-converter-cpp are not tested -->

# Usage

`go get -u github.com/awused/wallpapers`

Install with `go install -ldflags -H=windowsgui github.com/awused/wallpapers` on Windows to avoid spawning a visible console Window. <!-- TODO - include windows registry hacks in this repo -->

Fill in wallpapers.toml and copy it to your choice of /usr/local/etc/wallpapers.toml, /usr/etc/wallpapers.toml, $GOBIN/wallpapers.toml, or $HOME/.wallpapers.toml.

Run `wallpapers sync` to prepopulate the cache for your current set of wallpapers. This can be a very time consuming operation and stresses both your CPU and GPU. It can take hours to run for hundreds or thousands of images. The cache can take considerably more space than your original wallpapers, especially with high resolution images, so make sure there is sufficient disk space.

`wallpapers -h` will display additional usage information.

## Commands
### Random

`wallpapers random`

The random command will set one random wallpaper randomly on each monitor. It favours less recently selected wallpapers (See [go-strpick](/awused/go-strpick)) and will not select the same wallpaper for multiple monitors at the same time when there are enough wallpapers to avoid it.

If one of the selected wallpapers hasn't been cached it will perform the same upscaling and caching as sync. If you're running this as part of a periodic task or cron job this can interrupt whatever you are doing by stressing your GPU, so it's recommended to run sync manually so you can control the timing.

The --unlocked flag can be used to avoid changing wallpapers when the screen is locked.

### Preview

`wallpapers preview wallpaper.jpg`

The preview command is used to show you what a wallpaper will look like after processing. Using different flags it's possible to set all the same image manipulation values that are available in the config file, allowing you to dial in the specific settings you want. See `wallpapers preview -h` for more details.


### Sync

`wallpapers sync`

The sync will prepopulate the cache for the currently selected set of monitors and remove any invalid cache entries. It will also remove entries from the usage database, which is not done as part of normal operation. As stated above this can be a time consuming and CPU/GPU intensive process, but once the initial sync completes subsequent runs will only touch modified files.

One thing sync does not do is remove cached images for monitors that are no longer attached. If you have a laptop that you connect periodically to a 4K monitor, those 4K images will be untouched. You'll need to manually delete them if you've permanently stopped using monitors of a specific resolution.


# Image Manipulation

One of the biggest annoyances is dealing with different aspect ratios between your monitors, making some images look bad on some monitors, or images that just don't work well as wallpapers because they're too wide or too tall. Using different settings it's possible to crop, letterbox, or offset your wallpapers differently for all of your different aspect ratios.

Settings are all set per aspect ratio. So all 16:9 monitors, regardless of their actual resolution, will use the same settings for the same wallpapers. The configuration format is also explained in wallpapers.toml.

# Cropping/Letterboxing
The Top, Bottom, Left, and Right values are integers that control how many pixels are cropped from the original image. Negative integers result in padding.

Background is the colour used when padding images. It defaults to black. It can be any string recognized by ImageMagick as a [valid colour](https://www.imagemagick.org/script/color.php).


# Offsets
Using Vertical or Horizontal values, as decimal percentages (due to TOML limitations they must include a decimal component) can give you fine-grained control over exactly how much you translate an image up/down or right/left.

You can only specify Horizontal or Vertical, not both at once, and any offset can instead be done using a crop. Offsets can be slightly more efficient than cropping, but not substantially. <!-- TODO - until I implement interactive previews, where they'll be massively more efficient than cropping -->

