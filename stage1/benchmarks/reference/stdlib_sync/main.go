package main

import (
	"fmt"
	"sync"
)

func main() {
	counter := 1
	var mutex sync.Mutex
	mutex.Lock()
	counter = 2
	mutex.Unlock()
	mutex.Lock()
	fmt.Println(counter)
	mutex.Unlock()

	var ready string
	var once sync.Once
	once.Do(func() { ready = "configured" })
	fmt.Println(ready != "")

	var empty *int
	var emptyOnce sync.Once
	_ = emptyOnce
	if empty == nil {
		fmt.Println("empty")
	}

	channel := make(chan string, 1)
	channel <- "message"
	select {
	case message := <-channel:
		fmt.Println(message)
	default:
		fmt.Println("missing")
	}
}
