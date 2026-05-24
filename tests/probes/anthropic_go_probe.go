package main

import (
	"bytes"
	"fmt"
	"io"
	"net/http"
	"os"
)

func envOr(key, fallback string) string {
	if value := os.Getenv(key); value != "" {
		return value
	}
	return fallback
}

func main() {
	url := envOr("OPENPUP_ANTHROPIC_URL", "https://coding.dashscope.aliyuncs.com/apps/anthropic/v1/messages")
	apiKey := os.Getenv("OPENPUP_ANTHROPIC_API_KEY")
	model := envOr("OPENPUP_ANTHROPIC_MODEL", "qwen3.6-plus")

	if apiKey == "" {
		panic("OPENPUP_ANTHROPIC_API_KEY is required")
	}

	body := []byte(fmt.Sprintf(`{
  "model": %q,
  "messages": [
    { "role": "user", "content": "hi" }
  ],
  "max_tokens": 8192,
  "stream": false,
  "system": [
    { "type": "text", "text": "You are a coding assistant." }
  ]
}`, model))

	req, err := http.NewRequest(http.MethodPost, url, bytes.NewReader(body))
	if err != nil {
		panic(err)
	}

	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "*/*")
	req.Header.Set("x-api-key", apiKey)
	req.Header.Set("anthropic-version", "2023-06-01")

	transport := &http.Transport{
		ForceAttemptHTTP2: true,
		Proxy:             http.ProxyFromEnvironment,
	}
	client := &http.Client{Transport: transport}

	resp, err := client.Do(req)
	if err != nil {
		panic(err)
	}
	defer resp.Body.Close()

	data, _ := io.ReadAll(resp.Body)
	fmt.Printf("status=%d\n%s\n", resp.StatusCode, string(data))
}
