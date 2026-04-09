# Benchmarking Plan

## 📊 Step 1: Formal Benchmarking (Gathering Data)

You need precise numbers for your paper. "About 10MB" isn't enough; you need a table.

**Action:** Run a comparison test and record these 3 metrics:

*   **Compression Ratio:** (Original Size / Compressed Size)
*   **Compression Speed:** (MB per second)
*   **Decompression Speed:** (MB per second)

### Create a table like this:

| Method          | File Size (MB) | Ratio | Time (s) |
| :-------------- | :------------- | :---- | :------- |
| Original        | 700 MB         | 1.0x  | -        |
| GZIP (Standard) | ~? MB          | ?x    | ? s      |
| ZSTD (Fast)     | ~? MB          | ?x    | ? s      |
| SALC (Yours)    | ? MB           | ?x    | ? s      |

> **Why?** If SALC is slower but smaller, it's a "Storage-Optimized" tool. If it's faster AND smaller, it's a "State-of-the-art" tool.

## ⚡ Step 2: Optimization (Squeezing the Stone)

Now that you have a baseline, we can implement System-Specific Optimizations to widen the gap between you and ZIP.

### Idea A: Delta Encoding for Timestamps

*   **Current:** You likely store timestamps as 64-bit integers (e.g., `1697000001`, `1697000002`...). This is redundant.
*   **Optimization:** Store the difference between them.
    *   `1697000000` (Base)
    *   `+1`, `+1`, `+5`, `+2` (Deltas)
*   **Impact:** Small integers compress much better than large ones. This usually shaves off another 5-10%.

### Idea B: The "Integer Split"

*   **Current:** Your variables column is a `Vec<String>`. It mixes "User123" (Text) with "404" (Number).
*   **Optimization:** Detect if a variable is a pure number. If yes, put it in a separate `Vec<i64>`.
*   **Impact:** Compressing a binary array of integers is far more efficient than compressing their string representations.

## 🔍 Step 3: The "Killer Feature" (Search without Decompression)

This is what will get your paper accepted. Standard ZIP requires you to decompress the entire file to find one line. SALC does not.

### The Concept:
Because you separated the IDs (Templates), you can search for "Login Failed" errors by just scanning the tiny ID column.

### Implementation Plan:

1.  **User query:** `grep "Login Failed" file.salc`
2.  **SALC Tool:**
    *   Looks up "Login Failed" in the Registry -> Finds it is ID #4.
    *   Scans only the ID Column for the number 4.
    *   Decompresses only the chunks where ID=4 exists.
3.  **Result:** Massive speedup compared to `zgrep`.

## 🟢 Decision Point

Which path do you want to take right now?

*   **Strict Benchmarking:** "Let's run the tests and generate the graphs for the report immediately."
*   **Optimization:** "Let's implement Delta Encoding to make the file even smaller."
*   **The Killer Feature:** "Let's write a search function to prove we are faster than GZIP at querying."