package main

import (
	"fmt"
	"time"
)

func main() {
	start := time.Now()
	fmt.Println(start.UnixMilli() > 0)
	fmt.Println(time.Now().UnixMilli() > 0)
	time.Sleep(0)
	fmt.Println(true)
	elapsed := time.Since(start).Milliseconds()
	fmt.Println(elapsed == elapsed)
}
