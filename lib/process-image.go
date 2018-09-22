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
	"log"
	"math"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"sync"
	"time"

	"golang.org/x/image/bmp"
	//"strings"
	"syscall"
)

var gpuLock sync.Mutex

var closeChan = make(chan struct{})

var ErrStopped = errors.New("Processing stopped with StopGPU")

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
			// Lock the "GPU" even if we're CPU scaling.
			// You're not going to do more than one model at a time on any CPU.
			gpuLock.Lock()
			select {
			case <-closeChan:
				gpuLock.Unlock()
				return ErrStopped
			default:
			}

			if c.Waifu2xCaffe != nil {
				err = caffeProcess(validInFile, tempFile, scale, po.Denoise, c)
			} else {
				err = w2xcppProcess(validInFile, tempFile, scale, po.Denoise, c)
			}
			gpuLock.Unlock()
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
	// Not copying it would also be fine
	newimg := *img
	newimg.Width -= co.Left + co.Right
	newimg.Height -= co.Top + co.Bottom

	croppedFile := filepath.Join(tdir, hashPath(inFile)+cropOrPadStr+"-cropped.bmp")

	if fileExists(croppedFile) {
		return croppedFile, img, nil
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
		"-flatten",
		croppedFile)

	cmd := exec.Command(c.ImageMagick, args...)
	cmd.SysProcAttr = sysProcAttr
	err = cmd.Run()
	if err != nil {
		return "", nil, err
	}

	return croppedFile, &newimg, nil
}

// The two models to try from waifu2x caffe, the first one is much faster
// but can easily run out of memory
const model = `models/upconv_7_anime_style_art_rgb`
const backupModel = `models/anime_style_art_rgb`

func caffeProcess(
	inFile, outFile AbsolutePath, scale int, denoise bool, c *Config) error {
	err := caffeProcessModel(inFile, outFile, scale, denoise, c, model)
	if exitErr, ok := err.(*exec.ExitError); ok {
		if status, ok := exitErr.Sys().(syscall.WaitStatus); ok {

			// Probably Windows specific, but anyone running waifu2x-caffe on Linux
			// can figure this out themselves.
			if status.ExitStatus() == 3221225477 {
				log.Printf(
					"Access violation when running waifu2x on file [%s], "+
						"retrying with backup model\n", inFile)
				err = caffeProcessModel(
					inFile, outFile, scale, denoise, c, backupModel)
			}
		}
	}

	return err
}

func caffeProcessModel(
	inFile, outFile AbsolutePath,
	scale int,
	denoise bool,
	c *Config,
	modelPath string) error {

	if scale < 1 {
		return fmt.Errorf("Cannot use waifu2x with a scale less than 1")
	}
	// Not necessary anymore, keep commented to remind me if something breaks
	//wd := filepath.Dir(w2x)
	mode := "noise_scale"
	if scale == 1 {
		mode = "noise"
	} else if !denoise {
		mode = "scale"
	}

	args := []string{
		"-m", mode, "-i", inFile, "-o", outFile, "--model_dir", modelPath}
	if scale != 1 {
		args = append(args, "-s", strconv.Itoa(scale))
	}

	if denoise {
		args = append(args, "-n", "1")
	}

	if c.CPUScale {
		args = append(args, "-p", "cpu")
	}

	cmd := exec.Command(*c.Waifu2xCaffe, args...)
	//cmd.Dir = wd
	cmd.SysProcAttr = sysProcAttr
	err := cmd.Run()
	if err != nil {
		// CUDA will occasionally fail to initialize if the GPU is still busy after
		// the last call. Retry after a short delay.
		time.Sleep(5 * time.Second)
		cmd := exec.Command(*c.Waifu2xCaffe, args...)
		cmd.Stdout = os.Stdout
		cmd.Stderr = os.Stderr
		//cmd.Dir = wd
		cmd.SysProcAttr = sysProcAttr
		err = cmd.Run()
		if err != nil {
			log.Printf("Failed twice with settings: %v\n", args)
		}
	}
	return err
}

func w2xcppProcess(
	inFile, outFile AbsolutePath, scale int, denoise bool, c *Config) error {
	if scale < 1 {
		return fmt.Errorf("Cannot use waifu2x with a scale less than 1")
	}
	mode := "noise_scale"
	if scale == 1 {
		mode = "noise"
	} else if !denoise {
		mode = "scale"
	}

	// Force OpenCL to avoid CUDA, which is currently (2018-08)
	// broken in waifu2x-converter-cpp
	args := []string{
		"-m", mode,
		"-i", inFile,
		"-o", outFile,
		"--force-OpenCL",
		"--model_dir", c.Waifu2xCPPModels}
	if scale != 1 {
		args = append(args, "--scale_ratio", strconv.Itoa(scale))
	}

	if denoise {
		args = append(args, "--noise_level", "1")
	}

	if c.CPUScale {
		args = append(args, "--disable-gpu")

		if c.CPUThreads != nil {
			args = append(args, "-j", strconv.Itoa(*c.CPUThreads))
		}
	}

	cmd := exec.Command(c.Waifu2xCPP, args...)
	cmd.SysProcAttr = sysProcAttr
	err := cmd.Run()
	if err != nil {
		// Uncertain if CUDA and OpenCL run into the same problem, but try anyway
		time.Sleep(5 * time.Second)
		cmd := exec.Command(c.Waifu2xCPP, args...)
		cmd.Stdout = os.Stdout
		cmd.Stderr = os.Stderr
		cmd.SysProcAttr = sysProcAttr
		err = cmd.Run()
		if err != nil {
			log.Printf("Failed twice with settings: %v\n", args)
		}
	}

	if err == nil {
		// Workaround for https://github.com/DeadSix27/waifu2x-converter-cpp/issues/49 on Windows
		// This is fixed by that PR, but no new release has been produced in ten
		// months. I don't want to require users to build it themselves, so detect
		// and work around the bug.

		if _, err := os.Stat(outFile); err != nil {
			if !os.IsNotExist(err) {
				return err
			}

			if _, err := os.Stat(outFile + ".png"); err != nil {
				return err
			}

			err = os.Rename(outFile+".png", outFile)
		}
	}

	return err
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

	f := filepath.Join(tdir, hashPath(po.Input)) + "-" +
		strconv.Itoa(scale) + "-" + cropOrPadStr +
		"-intermediate.bmp"

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
				// Use the fastest compression settings
				"-define", "png:compression-level=0",
				// Huffman over zlib for anime
				"-define", "png:compression-strategy=2",
				inFile, convertedFile)
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
