package main

import "C"
import (
	"os"

	// "github.com/urfave/cli/v2" // might need this depending on cloudflared's entrypoint
)

//export StartTunnel
func StartTunnel(token *C.char) {
	go func() {
		os.Args = []string{"cloudflared", "tunnel", "--no-autoupdate", "run", "--token", C.GoString(token)}
		// We need to find the correct entry point for cloudflared's app
	}()
}

func main() {}
