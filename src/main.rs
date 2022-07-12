use anyhow::Result;
use gst::prelude::*;
use portal_screencast::ScreenCast;

mod avgfilter {
    mod imp {
        use byte_slice_cast::*;
        use gst_video::{subclass::prelude::*, VideoFrameRef};
        use once_cell::sync::Lazy;

        #[derive(Default)]
        pub struct AvgFilter;

        #[glib::object_subclass]
        impl ObjectSubclass for AvgFilter {
            const NAME: &'static str = "RsAvgFilter";
            type Type = super::AvgFilter;
            type ParentType = gst_video::VideoFilter;
        }

        impl ObjectImpl for AvgFilter {}
        impl GstObjectImpl for AvgFilter {}

        impl ElementImpl for AvgFilter {
            fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
                static ELEMENT_METADATA: Lazy<gst::subclass::ElementMetadata> = Lazy::new(|| {
                    gst::subclass::ElementMetadata::new(
                        "Avg Filter",
                        "Visualization",
                        "An avg filter",
                        "me (someone@example.com)",
                    )
                });

                Some(&*ELEMENT_METADATA)
            }

            fn pad_templates() -> &'static [gst::PadTemplate] {
                static PAD_TEMPLATES: Lazy<Vec<gst::PadTemplate>> = Lazy::new(|| {
                    let caps = gst::Caps::builder("video/x-raw")
                        .field("format", "RGBx")
                        .build();

                    vec![
                        gst::PadTemplate::new(
                            "sink",
                            gst::PadDirection::Sink,
                            gst::PadPresence::Always,
                            &caps,
                        )
                        .unwrap(),
                        gst::PadTemplate::new(
                            "src",
                            gst::PadDirection::Src,
                            gst::PadPresence::Always,
                            &caps,
                        )
                        .unwrap(),
                    ]
                });

                PAD_TEMPLATES.as_ref()
            }
        }

        impl BaseTransformImpl for AvgFilter {
            const MODE: gst_base::subclass::BaseTransformMode =
                gst_base::subclass::BaseTransformMode::AlwaysInPlace;
            const PASSTHROUGH_ON_SAME_CAPS: bool = false;
            const TRANSFORM_IP_ON_PASSTHROUGH: bool = true;
        }

        impl VideoFilterImpl for AvgFilter {
            fn transform_frame_ip(
                &self,
                _element: &Self::Type,
                frame: &mut VideoFrameRef<&mut gst::BufferRef>,
            ) -> Result<gst::FlowSuccess, gst::FlowError> {
                let buffer = frame.buffer_mut();
                let mut map = buffer.map_writable().unwrap();

                let data = map.as_mut_slice_of::<[u8; 4]>().unwrap();
                let avg = avgcolor(data);
                data.fill(avg);

                Ok(gst::FlowSuccess::Ok)
            }
        }

        fn avgcolor(pixels: &[[u8; 4]]) -> [u8; 4] {
            let mut total_r = 0;
            let mut total_g = 0;
            let mut total_b = 0;
            let mut total_a = 0;

            for &[r, g, b, a] in pixels {
                total_r += r as u32;
                total_g += g as u32;
                total_b += b as u32;
                total_a += a as u32;
            }

            let size = pixels.len() as u32;

            total_r /= size;
            total_g /= size;
            total_b /= size;
            total_a /= size;

            [total_r as u8, total_g as u8, total_b as u8, total_a as u8]
        }
    }

    glib::wrapper! {
        pub struct AvgFilter(ObjectSubclass<imp::AvgFilter>) @extends gst_video::VideoFilter, gst_base::BaseTransform, gst::Element, gst::Object;
    }

    impl AvgFilter {
        pub fn new(name: Option<&str>) -> AvgFilter {
            glib::Object::new(&[("name", &name)]).expect("Failed to create avg filter")
        }
    }
}

fn main() -> Result<()> {
    env_logger::init();

    ///// XDG STUFF /////
    let screencast = ScreenCast::new()?.start(None)?;

    let fd = screencast.pipewire_fd();
    let stream = screencast.streams().next().unwrap();
    let node = stream.pipewire_node();

    ///// GSTREAMER STUFF /////
    gst::init()?;

    let pipeline = gst::Pipeline::new(None);

    let pipewiresrc =
        gst::ElementFactory::make_with_properties("pipewiresrc", &[("fd", &fd), ("path", &node)])?;
    let avgfilter = avgfilter::AvgFilter::new(None);
    let videoconvert = gst::ElementFactory::make("videoconvert", None)?;
    let autovideosink = gst::ElementFactory::make("autovideosink", None)?;

    pipeline.add_many(&[
        &pipewiresrc,
        avgfilter.upcast_ref(),
        &videoconvert,
        &autovideosink,
    ])?;
    gst::Element::link_many(&[
        &pipewiresrc,
        avgfilter.upcast_ref(),
        &videoconvert,
        &autovideosink,
    ])?;

    log::info!("Setting state of pipeline to PLAYING");
    pipeline.set_state(gst::State::Playing)?;
    log::info!("Set state of pipeline to PLAYING");

    let bus = pipeline.bus().unwrap();

    for msg in bus.iter_timed(gst::ClockTime::NONE) {
        match msg.view() {
            gst::MessageView::Eos(..) => break,
            gst::MessageView::Error(err) => {
                log::error!("{err:?}");

                break;
            }
            gst::MessageView::StateChanged(st) => log::info!(
                "State changed: {}: {:?} -> {:?}",
                st.src().unwrap(),
                st.old(),
                st.current(),
            ),
            _ => (),
        }
    }

    pipeline.set_state(gst::State::Null)?;

    Ok(())
}
