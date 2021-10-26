package changewallpaperlib

import (
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"hash/crc32"
	"image"
	_ "image/gif"
	_ "image/jpeg"
	_ "image/png"
	"math"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"

	upscale "github.com/awused/aw-upscale"
	"golang.org/x/image/bmp"
	//"strings"
)

var upscaleSem chan struct{}

var closeChan = make(chan struct{})

var ErrStopped = errors.New("Processing stopped with StopGPU")

// Options to write a PNG as fast as possible without decent compression
// When used on tmpfs in memory this is naturally the right choice.
var fastPNGArgs = []string{
	"-define", "png:compression-level=0",
	"-define", "png:compression-strategy=4",
	"-define", "png:compression-filter=2",
}

// Stops doing any additional processing on the GPU
// Will wait for any ongoing actions to finish
func StopGPU() {
	close(closeChan)
}

// Offset arguments for cropping, expressed as positive or negative percentages
// of the image. Setting both at once is a mistake, right now.
type ImageProps struct {
	Vertical   float64 // Note that +vertical is up, not down
	Horizontal float64
	// Cropping/padding the original image
	// Values are pixels of the original image
	// Positive values crop, negative values pad
	Top    int
	Bottom int
	Left   int
	Right  int
	// Has to be something ImageMagick understands as a colour
	// blank means black padding
	// https://www.imagemagick.org/script/color.php
	Background string
}

func (co ImageProps) cropOrPadString() string {
	if co.Top == 0 && co.Bottom == 0 && co.Left == 0 && co.Right == 0 {
		return ""
	}
	// co.Background doesn't matter if all of the values are zero

	colour := ""
	if co.Background != "" {
		// Colour can be any string, potentially unsafe for use in filenames
		// Rather than escaping it and risking ambiguity (poor libraries in Go for
		// escaping file paths) just take the CRC32 as base32hex
		// The risk of collisions is low enough to not worry about
		colour =
			strconv.FormatUint(uint64(crc32.ChecksumIEEE([]byte(co.Background))), 32)
	}

	return fmt.Sprintf(
		"%d,%d,%d,%d,%s", co.Top, co.Bottom, co.Left, co.Right, colour)
}

func (co ImageProps) offsetString() string {
	if co.Vertical != 0 || co.Horizontal != 0 {
		return fmt.Sprintf("%.6f,%.6f", co.Horizontal, co.Vertical)
	}
	return ""
}

func (co ImageProps) String() string {
	cropOrPadStr := co.cropOrPadString()
	offsetStr := co.offsetString()

	if cropOrPadStr != "" && offsetStr != "" {
		return cropOrPadStr + "," + offsetStr
	} else if cropOrPadStr != "" {
		return cropOrPadStr
	} else {
		return offsetStr
	}
}

type ProcessOptions struct {
	Input  AbsolutePath
	Output AbsolutePath
	Width  int
	Height int
	// Default is "Fill", set touch to true to only touch the insides of the
	// target rectangle
	Touch bool
	// Denoise the image. Recommended to always be true as many anime images,
	// even pngs, contain undesired noise
	// Should be true unless Input is a partially processed intermediate file
	Denoise bool
	// Flatten transparency. Generally good to set this to tue for wallpapers
	// TODO -- Use the ImageProps background colour, but default to white instead
	Flatten bool
	// Whether to apply the Cropping/Padding settings in ImageProps
	// Should be true unless Input is a partially processed intermediate file
	CropOrPad  bool
	ImageProps ImageProps
}

func validateProcessOptions(po ProcessOptions) bool {
	return !(po.Input == "" || po.Output == "" || po.Width == 0 || po.Height == 0)
}

// Default mode is to completely fill the target width and height and crop, touch=true only scales the image to touch the insides of the rectangle
// Set denoise=false when the image has already been denoised. Don't double the same image multiple times, instead call this once for each necessary scaling factor
func ProcessImage(po ProcessOptions) error {
	if !validateProcessOptions(po) {
		return fmt.Errorf("ProcessOptions missing required field")
	}

	c, err := GetConfig()
	if err != nil {
		return err
	}

	validInFile, img, err := getImageConfig(po.Input)
	if err != nil {
		return err
	}

	// Must be done before other steps so img is set correctly
	if po.CropOrPad {
		validInFile, img, err = doCropOrPad(validInFile, po, img, c)
		if err != nil {
			return err
		}
	}

	err = createMissingDirectories(po.Output)
	if err != nil {
		return err
	}

	// Waifu2x only multiplies by powers of 2 and does not downscale so round up
	// to the nearest non-negative power of 2
	scale := getScalingFactorIgnoreCrop(po, img)

	if scale > 1 || po.Denoise {
		tempFile, err := getScaledIntermediateFile(po, scale)
		if err != nil {
			return err
		} // Should not be possible

		if !fileExists(tempFile) {
			select {
			case upscaleSem <- struct{}{}:
			case <-closeChan:
				return ErrStopped
			}

			upscaler := upscale.New(c.AlternateUpscaler).SetDenoise(po.Denoise).SetScale(scale)

			upscaler.Process(validInFile, tempFile)

			<-upscaleSem
		}

		if err != nil {
			return err
		}
		return imResize(tempFile, po.Output, po, img, c)
	} else {
		return imResize(validInFile, po.Output, po, img, c)
	}
}

func doCropOrPad(
	inFile AbsolutePath, po ProcessOptions, img *image.Config, c *Config) (
	AbsolutePath, *image.Config, error) {

	co := po.ImageProps
	cropOrPadStr := co.cropOrPadString()
	if cropOrPadStr == "" {
		return inFile, img, nil
	}

	tdir, err := TempDir()
	if err != nil {
		return "", nil, err
	}

	// This copies the color.Model interface pointer but we never modify that so
	// it's safe enough
	// Not copying it would have been less error prone
	newimg := *img
	newimg.Width -= co.Left + co.Right
	newimg.Height -= co.Top + co.Bottom

	croppedFile := filepath.Join(
		tdir, hashPath(inFile)+cropOrPadStr+"-cropped.png")

	if fileExists(croppedFile) {
		return croppedFile, &newimg, nil
	}

	bg := co.Background
	if bg == "" {
		bg = "black"
	}

	cropStr := fmt.Sprintf(
		"%dx%d%+d%+d!",
		newimg.Width,
		newimg.Height,
		co.Left,
		co.Top)

	args := append(getBaseConvertArgs(c),
		inFile,
		"-crop", cropStr,
		"-background", bg,
		// This will destroy any transparency, as will the transition to bmp
		// Users will need to set another BG colour if they have transparent images
		// that they want to crop this way
		"-flatten")

	args = append(args, fastPNGArgs...)
	args = append(args, croppedFile)

	cmd := exec.Command(c.ImageMagick, args...)
	cmd.SysProcAttr = sysProcAttr
	err = cmd.Run()
	if err != nil {
		return "", nil, err
	}

	return croppedFile, &newimg, nil
}

func getScaledIntermediateFile(
	po ProcessOptions, scale int) (AbsolutePath, error) {
	tdir, err := TempDir()
	if err != nil {
		return "", err
	}

	cropOrPadStr := ""
	if po.CropOrPad {
		cropOrPadStr = po.ImageProps.cropOrPadString()
	}

	// Using BMP here is faster but results in errors when upscaling
	// 16-bit PNGs. Rather than detecting those instances and converting them to 8-bit
	// images, just allow waifu2x to generate 16-bit output. Waifu2x does not
	// use expensive png compression methods.
	f := filepath.Join(tdir, hashPath(po.Input)) + "-" +
		strconv.Itoa(scale) + "-" + cropOrPadStr +
		"-intermediate.png"

	return f, nil
}

func GetScaledIntermediateFile(po ProcessOptions) (AbsolutePath, error) {
	scale, err := getScalingFactorApplyCrop(po)
	if err != nil {
		return "", err
	}

	return getScaledIntermediateFile(po, scale)
}

// Does not check po.CropOrPad
func getScalingFactorIgnoreCrop(po ProcessOptions, img *image.Config) int {
	xRatio := float64(po.Width) / float64(img.Width)
	yRatio := float64(po.Height) / float64(img.Height)

	ratio := 0.0
	if po.Touch {
		ratio = math.Min(xRatio, yRatio)
	} else {
		ratio = math.Max(xRatio, yRatio)
	}

	// Round ratio up to the next power of 2 that is at least 1
	// No risk of rounding, values are exact
	return int(math.Pow(
		2,
		math.Max(math.Ceil(math.Log2(ratio)), 0)))
}

// Does check and apply cropping or padding settings
func getScalingFactorApplyCrop(po ProcessOptions) (int, error) {
	_, img, err := getImageConfig(po.Input)
	if err != nil {
		return 0, err
	}

	if po.CropOrPad {
		img.Width -= po.ImageProps.Left + po.ImageProps.Right
		img.Height -= po.ImageProps.Top + po.ImageProps.Bottom
	}

	return getScalingFactorIgnoreCrop(po, img), nil
}

// Gets the image.Config of the input file, converting to png if necessary
func getImageConfig(inFile AbsolutePath) (string, *image.Config, error) {
	c, err := GetConfig()
	if err != nil {
		return "", nil, err
	}

	in, err := os.Open(inFile)
	if err != nil {
		return "", nil, err
	}
	defer in.Close()

	fi, err := in.Stat()
	if err != nil {
		return "", nil, err
	}

	if !fi.Mode().IsRegular() {
		return "", nil,
			fmt.Errorf("Input image [%s] is not a regular file", inFile)
	}

	// DecodeConfig should be more efficient than imagemagick's identify
	img, _, err := image.DecodeConfig(in)
	if err != nil && err != image.ErrFormat && err != bmp.ErrUnsupported {
		return "", nil, err
	}
	if err != nil {
		tdir, err := TempDir()
		if err != nil {
			return "", nil, err
		}

		// Go has rather limited image format support
		// try to use imagemagick to convert to a supported format
		// Don't use BMP because we don't want to clobber transparency yet
		convertedFile := filepath.Join(tdir, hashPath(inFile)+"-converted.png")
		// File might already exist from an earlier call
		inc, err := os.Open(convertedFile)
		if err != nil {
			args := append(
				getBaseConvertArgs(c),
				inFile, convertedFile)

			args = append(args, fastPNGArgs...)

			cmd := exec.Command(c.ImageMagick, args...)
			cmd.SysProcAttr = sysProcAttr
			err = cmd.Run()
			if err != nil {
				return "", nil, err
			}

			inc, err = os.Open(convertedFile)
		}

		if err != nil {
			return "", nil, err
		}
		defer inc.Close()

		fi, err = inc.Stat()
		if err != nil {
			return "", nil, err
		}

		if !fi.Mode().IsRegular() {
			return "", nil,
				fmt.Errorf("Input image [%s] is not a regular file", inFile)
		}

		img, _, err = image.DecodeConfig(inc)
		if err != nil {
			return "", nil, err
		}
		return convertedFile, &img, nil
	}

	return inFile, &img, nil
}

func imResize(inFile, outFile AbsolutePath, po ProcessOptions, img *image.Config, c *Config) error {
	resStr := fmt.Sprintf("%dx%d", po.Width, po.Height)
	modeStr := "^"
	if po.Touch {
		modeStr = ""
	}

	offsetString := "+0+0!"

	co := po.ImageProps

	if co.Horizontal != 0 || co.Vertical != 0 {
		// Choose the right offset scaling factor regardless of whether the image
		// is taller or wider than the monitor
		offsetScale := math.Max(
			float64(po.Width)/float64(img.Width),
			float64(po.Height)/float64(img.Height))

		// Multiple voff by negative 1 so that it's more intuitive in configs
		// Also Most of my offsets would be negative if I didn't
		voff := -1 * offsetScale * co.Vertical * float64(img.Height) / 100
		hoff := offsetScale * co.Horizontal * float64(img.Width) / 100

		offsetString = fmt.Sprintf("%+d%+d!", int(hoff), int(voff))
	}

	args := append(getBaseConvertArgs(c), inFile,
		"-filter", "Lanczos",
		"-resize", resStr+modeStr,
		"-gravity", "center",
		"-crop", resStr+offsetString)

	if po.Flatten {
		// Transparency appears to break when using waifu2x-caffe, so flatten
		// May be able to revisit in the future, but for now just flatten
		// This is very CPU intensive
		args = append(args, "-flatten")
	}

	args = append(args, outFile)

	cmd := exec.Command(c.ImageMagick, args...)
	cmd.SysProcAttr = sysProcAttr
	return cmd.Run()
}

// Used to avoid collisions when creating temporary files
// It's either this or created deep nested directories inside TempDir
// Which _might_ be a better option
func hashPath(path AbsolutePath) string {
	h := sha256.Sum256([]byte(path))
	return hex.EncodeToString(h[:])
}

func createMissingDirectories(outFile AbsolutePath) error {
	return os.MkdirAll(filepath.Dir(outFile), 0755)
}

func fileExists(file AbsolutePath) bool {
	_, err := os.Stat(file)
	return err == nil
}

// Convert to jpeg with quality = 100
// Should only be used when the PNG file size is too large for windows
// TODO -- Cleanup
/*func ConvertToJPEG(inFile, outFile string) error {
	c, err := GetConfig()
	if err != nil {
		return err
	}

	args := []string{"convert", inFile, "-quality", "100", outFile}
	cmd := exec.Command(c.ImageMagick, args...)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	return cmd.Run()
}*/

func getBaseConvertArgs(c *Config) []string {
	args := []string{}
	if c.ImageMagick7 {
		args = []string{"convert"}
	}

	args = append(args, "-define", "bmp:format=bmp3")

	return args
}
