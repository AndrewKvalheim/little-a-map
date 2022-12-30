# Testing

## File modification times

File modification times are a critical part of Little a Map’s inputs and outputs, but are not natively tracked by Git. As a workaround, modification times are stored in sidecar files and applied upon checkout using Git hooks:

```bash
ln --symbolic --relative scripts/{pre-commit,post-checkout} .git/hooks/
```
