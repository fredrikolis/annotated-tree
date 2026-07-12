// Concern: demo package exercising the Go symbol extractor | Non-concern: real server behavior (a fixture stub) | IO: (args) -> exit_code
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
