slint::include_modules!();

use std::{env, thread};

use anyhow::{bail, Result};

use gst::{
    glib::ControlFlow,
    prelude::*,
    ClockTime, SeekFlags, State,
};
use gst_video::VideoFrameExt;

use clap::{command, Parser};
use i_slint_backend_winit::WinitWindowAccessor;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    uri: String,

    #[arg(short, long)]
    zoom: f32,

    #[arg(short, long)]
    volume: f64,
}

fn try_gstreamer_video_frame_to_pixel_buffer(
    frame: &gst_video::VideoFrame<gst_video::video_frame::Readable>,
) -> Result<slint::SharedPixelBuffer<slint::Rgb8Pixel>> {
    match frame.format() {
        gst_video::VideoFormat::Rgb => {
            let slint_p_puffer = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::clone_from_slice(
                &frame.comp_data(0).unwrap(),
                frame.width(),
                frame.height(),
            );
            Ok(slint_p_puffer)
        }
        _ => {
            bail!(
                "Cannot convert frame to a slint RGB frame because it is format {}",
                frame.format().to_str()
            )
        }
    }
}

fn main() {
    // Setup Winit for the window backend
    slint::platform::set_platform(Box::new(i_slint_backend_winit::Backend::new().unwrap()))
        .expect("Error setting Winit as backend");

    // Parse the arguments
    let args = Args::parse();
    let uri = args.uri;
    let zoom = args.zoom;
    let volume = args.volume;

    // Setup the two loops, one for gstreamer and one for slint
    let app = App::new().unwrap();
    let g_main_loop = gst::glib::MainLoop::new(None, false);
    gst::init().expect("Error initializing gstreamer");

    // start setting up the pipeline
    let appsink = gst::ElementFactory::make("appsink")
        .name("sink")
        .build()
        .unwrap()
        .downcast::<gst_app::AppSink>()
        .expect("Error building appsink");

    let playbin = gst::ElementFactory::make("playbin3")
        .property("uri", uri)
        .property("volume", volume)
        .property("video-sink", &appsink)
        .build()
        .expect("Error building playbin");

    let bus = playbin.bus().unwrap();

    appsink.set_caps(Some(
        &gst_video::VideoCapsBuilder::new()
            .format(gst_video::VideoFormat::Rgb)
            .build(),
    ));

    // The app should update on each new sample
    appsink.set_callbacks({
        let app_weak = app.as_weak();
        gst_app::AppSinkCallbacks::builder()
            .new_sample(move |appsink| {
                let sample = appsink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                let buffer = sample
                    .buffer_owned()
                    .expect("Error getting buffer from sample");
                let caps = sample.caps().expect("Error getting caps from sample");
                let video_info =
                    gst_video::VideoInfo::from_caps(caps).expect("Error getting video info");
                let video_frame = gst_video::VideoFrame::from_buffer_readable(buffer, &video_info)
                    .expect("Couldn't build video frame");
                let slint_frame = try_gstreamer_video_frame_to_pixel_buffer(&video_frame)
                    .expect("Unable to convert the video frame to a slint video frame!");

                app_weak
                    .upgrade_in_event_loop(move |app| {
                        app.set_zoom(zoom);
                        app.set_video_width(video_info.width().try_into().unwrap());
                        app.set_video_height(video_info.height().try_into().unwrap());
                        app.set_video_frame(slint::Image::from_rgb8(slint_frame));
                    })
                    .expect("Error updating app in event loop");
                Ok(gst::FlowSuccess::Ok)
            })
            .build()
    });

    // The bus will watch for events
    let _bus_watch = bus
        .add_watch({
            let playbin_clone = playbin.clone();
            let app_weak = app.as_weak();
            let g_main_loop_clone = g_main_loop.clone();
            move |_bus, msg| {
                use gst::MessageView;

                match msg.view() {
                    MessageView::Eos(_eos) => {
                        // video loops by default when it ends
                        playbin_clone
                            .seek_simple(SeekFlags::FLUSH, ClockTime::ZERO)
                            .unwrap();
                        ControlFlow::Continue
                    }
                    MessageView::StateChanged(state_changed) => {
                        match state_changed.current() {
                            gst::State::VoidPending => (),
                            gst::State::Null => (),
                            gst::State::Ready => (),
                            gst::State::Paused => {
                                app_weak
                                    .upgrade_in_event_loop(|app| {
                                        app.set_playing(false);
                                    })
                                    .unwrap();
                            }
                            gst::State::Playing => {
                                app_weak
                                    .upgrade_in_event_loop(|app| {
                                        app.set_playing(true);
                                    })
                                    .unwrap();
                            }
                        }
                        ControlFlow::Continue
                    }
                    MessageView::Error(_error) => {
                        g_main_loop_clone.quit();
                        ControlFlow::Break
                    }
                    _ => ControlFlow::Continue,
                }
            }
        })
        .expect("Error in bus watch");

    // key event handler
    app.on_key_pressed({
        let app_weak = app.as_weak().unwrap();
        let playbin_clone = playbin.clone();
        let g_main_loop_clone = g_main_loop.clone();
        move |event| {
            let key = event.text.as_str();
            match key {
                "q" => {
                    playbin_clone
                        .set_state(gst::State::Null)
                        .expect("Error cleaning up playbin");
                    g_main_loop_clone.quit();
                    app_weak.window().hide().unwrap();
                }
                " " => {
                    let current = playbin_clone.current_state();
                    match current {
                        State::Paused => {
                            playbin_clone
                                .set_state(State::Playing)
                                .expect("Error playing video");
                        }
                        State::Playing => {
                            playbin_clone
                                .set_state(State::Paused)
                                .expect("Error pausing video");
                        }
                        _ => (),
                    }
                }
                _ => (),
            }
        }
    });

    app.on_toggle_state({
        let playbin_clone = playbin.clone();
        move || {
            let (_, state, _) = playbin_clone.state(ClockTime::NONE);
            match state {
                gst::State::VoidPending => todo!(),
                gst::State::Null => todo!(),
                gst::State::Ready => todo!(),
                gst::State::Playing => {
                    playbin_clone
                        .set_state(gst::State::Paused)
                        .expect("Error pausing video");
                }
                gst::State::Paused => {
                    playbin_clone
                        .set_state(gst::State::Playing)
                        .expect("Error playing video");
                }
            }
        }
    });

    app.on_replay({
        let playbin_clone = playbin.clone();
        move || {
            playbin_clone
                .seek_simple(SeekFlags::FLUSH, ClockTime::ZERO)
                .expect("Error replaying video")
        }
    });

    app.on_mouse_drag({
        let clone = app.as_weak();
        move || {
            let ui = clone.unwrap();
            ui.window().with_winit_window(|window| {
                window.drag_window().expect("Error moving window");
            });
        }
    });

    app.on_seek({
        let playbin_clone = playbin.clone();
        move |num, denom| {
            let duration: ClockTime = playbin_clone.query_duration().unwrap();
            let seek_time =
                duration.mul_div_round(u64::try_from(num).unwrap(), u64::try_from(denom).unwrap());
            playbin_clone
                .seek_simple(SeekFlags::FLUSH, seek_time)
                .expect("Error seeking video");
        }
    });

    playbin
        .set_state(gst::State::Playing)
        .expect("Error starting playbin");

    thread::spawn({
        let g_main_loop_clone = g_main_loop.clone();
        move || {
            g_main_loop_clone.run();
        }
    });

    app.window().on_close_requested(move || {
        playbin
            .set_state(gst::State::Null)
            .expect("Error cleaning up playbin");
        g_main_loop.quit();
        slint::CloseRequestResponse::HideWindow
    });

    app.run().expect("Error occurred while running app");
}
