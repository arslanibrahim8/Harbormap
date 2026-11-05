package notify

type Email struct{}

func (e Email) Notify(msg string) {
	send(msg)
}
