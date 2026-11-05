package web

import (
	"net/http"

	"example.com/shop"
)

type Server struct {
	Store    shop.OrderStore
	Notifier shop.Notifier
	mux      *http.ServeMux
}

func NewServer(st shop.OrderStore) *Server {
	s := &Server{Store: st, mux: http.NewServeMux()}
	s.routes()
	return s
}

func (s *Server) routes() {
	s.mux.HandleFunc("/orders", s.handleOrder)
}

func (s *Server) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	s.handleOrder(w, r)
}

func (s *Server) handleOrder(w http.ResponseWriter, r *http.Request) {
	o := &shop.Order{}
	if err := shop.Validate(o); err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}
	s.Store.Save(o)
	s.Notifier.Notify("saved")
	writeJSON(w, o)
}
