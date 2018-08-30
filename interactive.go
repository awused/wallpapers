package main

import "github.com/urfave/cli"

/*
Catch ctrl+c


Reset
Preview
Print v2:(for all monitors)
v2: Monitors {All, [resolutions]}
v2: SetPath
v3: AppendToConfig
v3: MoveToOriginalsFolder (do not overwrite anything)



Vertical
Horizontal
Top
Bottom
Left
Right
Background
*/

func interactiveCommand() cli.Command {
	cmd := cli.Command{}
	cmd.Name = "interactive"
	cmd.Usage = "Interactively preview a single image on every monitor to " +
		"quickly iterate on your settings."

	return cmd
}
