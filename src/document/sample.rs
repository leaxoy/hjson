/// Built-in sample shown on the home page, exercising every JSON type.
pub const TEXT: &str = r##"{
  "name": "hjson-viewer",
  "version": "0.1.1",
  "private": true,
  "repository": null,
  "keywords": ["json", "viewer", "gpui", "rust"],
  "stats": {
    "stars": 1024,
    "watches": 38.5,
    "downloads": 128000,
    "ratio": -0.75
  },
  "contributors": [
    {
      "name": "Lena",
      "email": "lena@example.com",
      "active": true,
      "commits": 512
    },
    {
      "name": "Marco",
      "email": null,
      "active": false,
      "commits": 96
    }
  ],
  "config": {
    "theme": "dark",
    "lineNumbers": true,
    "tabSize": 4,
    "fontFamily": "\"JetBrains Mono\", monospace",
    "nested": {
      "deep": {
        "path": [
          1,
          2,
          3,
          {
            "leaf": "值"
          }
        ]
      }
    }
  },
  "flags": [true, false, null]
}"##;
