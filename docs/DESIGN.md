# Design 

The following describes planned routing and caching work, not implemented behavior.

- Parses user request
- Applies inference on request parsing for prefix caching, "prefetch" operations prior to reduce token consumption, and use far more intelligent routing across different models
- Understand an incoming prompt before inference, exploit its structure to avoid redundant computation, and manage that state efficiently across a fleet of inference workers

```markdown
OpenAI-compatible request
        |
        v
+---------------------------+
|       Rust Gateway        |
|                           |
| chat canonicalization     |
| HF tokenizer/templates    |
| prefix block hashing      |
| semantic embedding        |
| cache-location index      |
| routing / scheduling      |
| admission policy          |
| observability             |
+-------------+-------------+
              |
      choose vLLM replica
              |
      +-------+-------+
      |               |
      v               v
   vLLM A          vLLM B
      |               |
      +------ + -------+
             |
          LMCache
             |
     CPU / NVMe / remote
```


1. **Receive an OpenAI-compatible chat/completion request in a Rust gateway.**

2. **Canonicalize and tokenize it.** Use Hugging Face `tokenizers`, which is itself Rust. For chat requests, respect the model's Hugging Face chat template rather than inventing formatting; Transformers stores those templates with the tokenizer/model.

3. **Break the token stream into prefix blocks and hash them.** Maintain an index of which vLLM replicas currently have which prefixes cached. This corresponds directly to how vLLM Automatic Prefix Caching works: its cache identity is hash-based over token blocks and their prefixes.

4. **Generate a semantic embedding of the request asynchronously**, preferably through Hugging Face Text Embeddings Inference. TEI is largely Rust infrastructure, does token-based dynamic batching, exposes production metrics/tracing, and supports current embedding models such as Qwen3.

5. Use the two representations for different purposes:

   - **token hashes → exact computational reuse / prefix-cache routing**
   - **embedding → semantic request indexing, workload analysis, and optionally a separately controlled semantic response cache**

   Do **not** use semantic similarity to claim two prompts can share KV cache. vLLM's prefix reuse requires the appropriate exact token prefix.

6. **Route the request to the vLLM replica with the greatest existing prefix overlap** rather than round-robin routing.

7. Then make the project substantially more interesting: implement a **content-addressed distributed KV-cache tier** so cached prefixes can survive outside one GPU process and be reused across workers.