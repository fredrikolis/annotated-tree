// Server: demo package exercising the Go extractor. | I/O: (args) -> exit_code
package main

type Handler struct {
	name string
}

func New(name string) *Handler {
	return &Handler{name: name}
}

func (h *Handler) Serve() error {
	return nil
}
