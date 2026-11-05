package main

import (
	"log"
	"net/http"

	"example.com/shop/http"
	"example.com/shop/store"
)

func main() {
	st := store.NewMemory()
	srv := web.NewServer(st)
	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		log.Printf("%s %s", r.Method, r.URL.Path)
		srv.ServeHTTP(w, r)
	})
	log.Fatal(http.ListenAndServe(":8080", handler))
}
