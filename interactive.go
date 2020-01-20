package main

import (
	"errors"
	"fmt"
	"log"
	"os"
	"os/signal"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"

	lib "github.com/awused/wallpapers/lib"
	prompt "github.com/c-bata/go-prompt"
	"github.com/urfave/cli"
)

/*
Print v2:(for all monitors)
v2: Monitors {All, [resolutions]}
v2: SetPath
v3: AppendToConfig
v3: MoveToOriginalsFolder (do not overwrite anything)
*/

func interactiveCommand() cli.Command {
	cmd := cli.Command{}
	cmd.Name = "interactive"
	cmd.Usage = "Interactively preview a single image on every monitor to " +
		"quickly iterate on your settings."
	cmd.ArgsUsage = "FILE"
	cmd.Before = beforeFunc

	cmd.Action = interactiveAction

	return cmd
}

func interactiveAction(c *cli.Context) error {
	if c.NArg() == 0 {
		checkErr(errors.New("Missing input file"))
	}

	w, err := filepath.Abs(c.Args().First())
	checkErr(err)

	// Large buffered channel so it doesn't block signals if it's busy
	sigs := make(chan os.Signal, 100)
	promptChan := make(chan struct{}, 1)
	inputChan := make(chan string)
	signal.Notify(sigs, syscall.SIGINT, syscall.SIGHUP)

	go func() {
		promptUntilDone(w, inputChan)
		promptChan <- struct{}{}
	}()

	for {
		select {
		case <-promptChan:
			return nil
		case <-sigs:
			// We need to make sure we clean up, so consume sigint
			inputChan <- "exit"
		}
	}
}

func completer(d prompt.Document) []prompt.Suggest {
	s := []prompt.Suggest{
		{Text: "exit", Description: "Exit the program"},
		{Text: "print", Description: "Print the settings to be copied into the" +
			" config file"},
		{Text: "reset", Description: "Reset all settings"},
		{Text: vertical, Description: "Set the veritical offset"},
		{Text: horizontal, Description: "Set the horizontal offset"},
		{Text: top, Description: "Set the cropping or padding value for" +
			" the top of the image"},
		{Text: bottom, Description: "Set the cropping or padding value for" +
			"the bottom of the image"},
		{Text: left, Description: "Set the cropping or padding value for" +
			"the left side of the image"},
		{Text: right, Description: "Set the cropping or padding value for" +
			" the right side of the image"},
		{Text: background, Description: "Set the background colour for padding"},
	}
	return prompt.FilterHasPrefix(s, d.TextBeforeCursor(), true)
}

// Just have to make everything as difficult as possible
func tomlDouble(f float64) string {
	s := fmt.Sprintf("%g", f)
	if !strings.ContainsRune(s, '.') {
		s = s + ".0"
	}
	return s
}

func printImageProps(ip lib.ImageProps) {
	if ip.Vertical != 0 {
		fmt.Printf("Vertical = %s\n", tomlDouble(ip.Vertical))
	}

	if ip.Horizontal != 0 {
		fmt.Printf("Horizontal = %s\n", tomlDouble(ip.Horizontal))
	}

	if ip.Top != 0 {
		fmt.Printf("Top = %d\n", ip.Top)
	}
	if ip.Bottom != 0 {
		fmt.Printf("Bottom = %d\n", ip.Bottom)
	}
	if ip.Left != 0 {
		fmt.Printf("Left = %d\n", ip.Left)
	}
	if ip.Right != 0 {
		fmt.Printf("Right = %d\n", ip.Right)
	}

	if ip.Background != "" && ip.Background != "black" {
		fmt.Printf("Background = '%s'\n", ip.Background)
	}
}

func setInt(toSet *int) func(string, string) {
	return func(s, p string) {
		input := strings.TrimPrefix(s, p)
		n, err := strconv.Atoi(input)
		if err != nil {
			fmt.Printf("Invalid input \"%s\"\n", input)
			return
		}
		*toSet = n
	}
}

func setDouble(toSet *float64) func(string, string) {
	return func(s, p string) {
		input := strings.TrimPrefix(s, p)
		n, err := strconv.ParseFloat(input, 64)
		if err != nil {
			fmt.Printf("Invalid input \"%s\"\n", input)
			return
		}
		*toSet = n
	}
}
func setString(toSet *string) func(string, string) {
	return func(s, p string) {
		input := strings.TrimPrefix(s, p)
		*toSet = input
	}
}

func promptUntilDone(wallpaper string, inputChan chan string) {
	imageProps := lib.ImageProps{}
	executors := map[string]func(string, string){
		vertical + " ":   setDouble(&imageProps.Vertical),
		"v ":             setDouble(&imageProps.Vertical),
		horizontal + " ": setDouble(&imageProps.Horizontal),
		"h ":             setDouble(&imageProps.Horizontal),
		top + " ":        setInt(&imageProps.Top),
		"t ":             setInt(&imageProps.Top),
		bottom + " ":     setInt(&imageProps.Bottom),
		"b ":             setInt(&imageProps.Bottom),
		left + " ":       setInt(&imageProps.Left),
		"l ":             setInt(&imageProps.Left),
		right + " ":      setInt(&imageProps.Right),
		"r ":             setInt(&imageProps.Right),
		background + " ": setString(&imageProps.Background),
		"bg ":            setString(&imageProps.Background),
	}

	exit := prompt.OptionAddKeyBind(prompt.KeyBind{
		Key: prompt.ControlC,
		Fn: func(b *prompt.Buffer) {
			inputChan <- "exit"
		},
	})

	monitors, err := lib.GetMonitors(false, false)
	checkErr(err)

	if len(monitors) == 0 {
		log.Println("No monitors detected.")
		return
	}

	fmt.Println("Previewing...")
	interactivePreview(wallpaper, imageProps, monitors)

PromptLoop:
	for {
		go func() {
			// prompt.Input is blocking, synchronous, and provides no way to abort it
			inputChan <- strings.ToLower(prompt.Input("> ", completer, exit))
		}()
		in := <-inputChan
		if in == "exit" {
			return
		}
		if in == "print" {
			printImageProps(imageProps)
			continue
		}

		if in == "reset" {
			imageProps = lib.ImageProps{}

			interactivePreview(wallpaper, imageProps, monitors)
			continue
		}

		// Very naive, but adequate
		for s, e := range executors {
			if strings.HasPrefix(in, s) {
				e(in, s)

				interactivePreview(wallpaper, imageProps, monitors)
				continue PromptLoop
			}
		}

		fmt.Println("Unknown command")
	}
}

func interactivePreview(w string, imageProps lib.ImageProps, monitors []*lib.Monitor) {
	defer func() {
		r := recover()
		if r != nil {
			fmt.Println("Unexpected error: ", r)
		}
	}()

	previewWallpaperUsingFakeCache(w, imageProps, monitors)
}
