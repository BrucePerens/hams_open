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
	"time"
)

var simulatorServer *http.Server
var simulatorListener net.Listener

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
func StartTunnel(token *C.char) {
	go func() {
		os.Args = []string{"cloudflared", "tunnel", "--no-autoupdate", "run", "--token", C.GoString(token)}
	}()
}

//export StopTunnel
func StopTunnel() {
}

func main() {}
