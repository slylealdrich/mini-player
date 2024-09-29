# mini-player

A little mini video player I made to play videos while I do work. Built using
gstreamer for video stuff and slint for the GUI. Some free icons from
FontAwesome.

## Build

You must have gstreamer installed on your system to run the application. To build just 
`cargo run` with arguments. You must specify a --uri, a --zoom level, and the --volume 
of the video.

## TODO

- Use OpenGL with slint to hopefully decrease extreme CPU and memory usage (?).

- Perhaps beautify it a bit more

- More fluid seeking

- Fix whatever is going on with some video types