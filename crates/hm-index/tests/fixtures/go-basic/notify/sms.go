package notify

import "fmt"

type SMS struct{}

func (s *SMS) Notify(msg string) {
	send("sms: " + msg)
}

func send(msg string) {
	fmt.Println(msg)
}
