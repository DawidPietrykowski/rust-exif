## `rust-exif`

Media management utility designed to select specific files matching provided filters and perform actions on them.

## Description

This tool was created to simplify photo/video management workflows. It allows you to pick files matching specific `Rating` tag criteria and manage them.

For example the following command:
```
rust-exif -s source_dir -d selected_image_dir -a -m -t 4 copy
```

Would select all photos and videos from the `source_dir` that contain an EXIF tag `Rating` with the value of at least `4` and copy them to the `selected_dir`.
It additionally matches raw files which means that in the case of there being a file matching the filter with a filename of `IMG.jpg` and another file `IMG.ARW` then it would also apply the same action to the `.ARW` file.

## Usage

```
Usage: rust-exif [OPTIONS] --src <SRC> <COMMAND>

Commands:
  move
  copy
  delete
  print
  help    Print this message or the help of the given subcommand(s)

Options:
  -t, --threshold <THRESHOLD>
          [default: 5]
  -i, --inverse

  -v, --verbose

  -d, --dest <DEST>

  -s, --src <SRC>

  -e, --exclude <EXCLUDE>

  -f, --flip-exclusion

  -m, --match-raws

  -a, --include-videos

  -l, --label <LABEL>

  -c, --comparison-command <COMPARISON_COMMAND>
          [default: more-equal] [possible values: more-equal, less-equal, equal]
  -h, --help
          Print help
```
