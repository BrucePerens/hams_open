// This file is part of hams_open, an open source module.
// SPDX-License-Identifier: AGPL-3.0-or-later

package main

import "C"
import (
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/tls"
	"crypto/x509"
	"crypto/x509/pkix"
	"fmt"
	"log"
	"math/big"
	"net"
	"net/http"
	"net/http/httputil"
	"net/url"
	"os"
	"os/exec"
	"sync"
	"time"
)

var simulatorServer *http.Server
var simulatorListener net.Listener

// Bug fix (2026-09-22): StartTunnel/StopTunnel below used to be a stub -- it set
// os.Args in a background goroutine and returned immediately without ever calling
// anything that actually starts a tunnel, so every caller (cloudflare_daemon.py's
// run_tunnel() loop) saw it return near-instantly forever, logged "exited
// unexpectedly, restarting immediately", and looped at ~1 call/second without ever
// establishing a real Cloudflare connection. Found live on hams1 2026-09-22 when
// this crash-looped for two different tunnels (hams.com's and an unrelated site's)
// simultaneously.
//
// The vendored daemons/cloudflared/cmd/cloudflared tree (the real, full cloudflared
// CLI source) DOES have a correct implementation of the actual tunnel-running logic
// (mainOriginal(), the same code every real `cloudflared tunnel run` invocation
// uses) -- but it lives in its own `package main`, and Go does not allow importing
// a "package main" as a library from another package main, so this file cannot
// call it in-process. Rather than duplicating that package's many files (main.go
// alone depends on sibling files like app_service.go, linux_service.go, etc., all
// in the same directory), this shells out to the real, now-working `cloudflared`
// binary built from that source (see daemons/cloudflared/README or the release
// runbook for how it's built and deployed) as a subprocess per tunnel -- the same
// approach Cloudflare's own docs recommend for embedding cloudflared in another
// program, and the conventional way to reuse a CLI tool's logic without forking
// its process model.
//
// Keyed by tunnel_key (the same opaque, non-secret Cloudflare tunnel id
// cloudflare_daemon.py already uses to key its own per-tunnel Python state) rather
// than a single global slot: "one Odoo server fronting several websites is the
// common case" (see cloudflare_daemon.py's own module-level comment), so this must
// support more than one concurrently running tunnel per process. A single global
// `*exec.Cmd` here would make a second real tunnel's StartTunnel call refuse to
// start (mistaking the first tunnel's process for its own), reproducing the same
// crash-loop symptom for whichever tunnel loses the race -- which is effectively
// what happened live even under the old stub, since neither one ever actually
// started; a real single-slot implementation would have hit this for real.
var (
	tunnelMu   sync.Mutex
	tunnelCmds = map[string]*exec.Cmd{}
)

//export StartLocalSimulator
func StartLocalSimulator(targetPort C.int) C.int {
	targetURL, err := url.Parse(fmt.Sprintf("http://127.0.0.1:%d", int(targetPort)))
	if err != nil {
		log.Printf("Failed to parse target URL: %v", err)
		return -1
	}

	proxy := httputil.NewSingleHostReverseProxy(targetURL)
	originalDirector := proxy.Director
	proxy.Director = func(req *http.Request) {
		originalDirector(req)
		req.Host = targetURL.Host
		req.Header.Set("CF-Connecting-IP", "127.0.0.1")
		req.Header.Set("X-Forwarded-For", "127.0.0.1")
		req.Header.Set("CF-Visitor", `{"scheme":"https"}`)
	}

	simulatorServer = &http.Server{Handler: proxy}

	// Generate self-signed cert
	priv, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		log.Printf("Failed to generate private key: %v", err)
		return -1
	}
	template := x509.Certificate{
		SerialNumber: big.NewInt(1),
		Subject: pkix.Name{
			Organization: []string{"Hams Open Simulator"},
		},
		NotBefore:             time.Now(),
		NotAfter:              time.Now().Add(time.Hour * 24),
		KeyUsage:              x509.KeyUsageKeyEncipherment | x509.KeyUsageDigitalSignature,
		ExtKeyUsage:           []x509.ExtKeyUsage{x509.ExtKeyUsageServerAuth},
		BasicConstraintsValid: true,
		IPAddresses:           []net.IP{net.ParseIP("127.0.0.1")},
	}
	derBytes, err := x509.CreateCertificate(rand.Reader, &template, &template, &priv.PublicKey, priv)
	if err != nil {
		log.Printf("Failed to create cert: %v", err)
		return -1
	}

	cert := tls.Certificate{
		Certificate: [][]byte{derBytes},
		PrivateKey:  priv,
	}

	simulatorServer.TLSConfig = &tls.Config{Certificates: []tls.Certificate{cert}}

	simulatorListener, err = net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		log.Printf("Failed to listen: %v", err)
		return -1
	}

	port := simulatorListener.Addr().(*net.TCPAddr).Port

	go func() {
		if err := simulatorServer.ServeTLS(simulatorListener, "", ""); err != nil && err != http.ErrServerClosed {
			log.Printf("Simulator server error: %v", err)
		}
	}()

	return C.int(port)
}

//export StopLocalSimulator
func StopLocalSimulator() {
	if simulatorServer != nil {
		simulatorServer.Close()
	}
}

//export StartTunnel
func StartTunnel(tunnelKey *C.char, token *C.char, binPath *C.char) {
	key := C.GoString(tunnelKey)
	tokenStr := C.GoString(token)
	bin := C.GoString(binPath)

	tunnelMu.Lock()
	if _, running := tunnelCmds[key]; running {
		tunnelMu.Unlock()
		log.Printf("StartTunnel(%s): already running in this process, refusing to start a second one", key)
		return
	}
	cmd := exec.Command(bin, "tunnel", "--no-autoupdate", "run", "--token", tokenStr) // burn-ignore-cloudflared-ingress
	// Bug fix (2026-09-22), confirmed live on hams1: this used to be
	// `cmd.Stdout = os.Stdout; cmd.Stderr = os.Stderr`. When this library is
	// loaded into odoo.service (a systemd unit), the PARENT process's fd 1/2
	// are a socket to journald, not a regular file. Handing that same socket
	// straight to a freshly exec'd Go binary made the CHILD's own Go runtime
	// fail at thread-creation time ("fatal error: runtime: cannot allocate
	// memory") -- confirmed by elimination: every other environmental
	// variable (RLIMIT_AS, systemd sandboxing directives, mount/pid/net
	// namespaces, being loaded via ctypes from Python) was reproduced
	// individually and in combination with a direct binary invocation or a
	// minimal ctypes repro script, and none of them reproduced the crash --
	// only spawning from the real odoo.service process (journald-socket fd
	// 1/2) did, and only this fix (writing to a real, regular file instead)
	// made it start reliably. Written under /var/log/odoo (already one of
	// odoo.service's own ReadWritePaths) rather than /tmp, since PrivateTmp=
	// on that unit makes /tmp private per-service-restart and hard for an
	// operator to find.
	logPath := fmt.Sprintf("/var/log/odoo/cloudflared-%s.log", key)
	logFile, logErr := os.OpenFile(logPath, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0644)
	if logErr != nil {
		log.Printf("StartTunnel(%s): could not open %s for the subprocess's output: %v", key, logPath, logErr)
	} else {
		cmd.Stdout = logFile
		cmd.Stderr = logFile
		defer logFile.Close()
	}
	if err := cmd.Start(); err != nil {
		tunnelMu.Unlock()
		log.Printf("StartTunnel(%s): failed to start cloudflared binary %s: %v", key, bin, err)
		return
	}
	tunnelCmds[key] = cmd
	tunnelMu.Unlock()

	// Blocks until the subprocess exits. The Python caller (run_tunnel() in
	// cloudflare_daemon.py) calls this in a loop, once per tunnel key, and treats
	// a return from this function as "the tunnel exited, pause a second and
	// restart it" -- so this must not return until the real tunnel process does.
	_ = cmd.Wait()

	tunnelMu.Lock()
	delete(tunnelCmds, key)
	tunnelMu.Unlock()
}

//export StopTunnel
func StopTunnel(tunnelKey *C.char) {
	key := C.GoString(tunnelKey)
	tunnelMu.Lock()
	cmd, running := tunnelCmds[key]
	tunnelMu.Unlock()
	if running && cmd.Process != nil {
		// SIGINT, not SIGKILL: mainOriginal()'s own cli app treats an interrupt as
		// a request for graceful shutdown (closing connections cleanly), the same
		// as a real operator pressing Ctrl-C on `cloudflared tunnel run`.
		_ = cmd.Process.Signal(os.Interrupt)
	}
}

func main() {}
