package store

import "testing"

func TestSave(t *testing.T) {
	m := NewMemory()
	if err := m.Save(nil); err != nil {
		t.Fatal(err)
	}
	helper(t)
}

func helper(t *testing.T) {}
