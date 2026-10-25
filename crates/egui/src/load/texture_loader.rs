use emath::Vec2;

use super::{
    BytesLoader as _, Context, HashMap, ImagePoll, Mutex, SizeHint, SizedTexture, TextureHandle,
    TextureLoadResult, TextureLoader, TextureOptions, TexturePoll,
};

/// SVG:s might have several different sizes loaded
type Bucket = HashMap<Option<SizeHint>, Entry>;

struct Entry {
    last_used: u64,

    /// Size of the original SVG, if any, or the texel size of the image if not an SVG.
    source_size: Vec2,

    handle: TextureHandle,
}

#[derive(Default)]
struct State {
    pass_index: u64,

    /// By URI, then by texture options (an image is mostly drawn with one). Looked up by the
    /// borrowed URI, so a texture already loaded costs no allocation.
    cache: HashMap<String, Vec<(TextureOptions, Bucket)>>,
}

#[derive(Default)]
pub struct DefaultTextureLoader {
    state: Mutex<State>,
}

impl TextureLoader for DefaultTextureLoader {
    fn id(&self) -> &'static str {
        crate::generate_loader_id!(DefaultTextureLoader)
    }

    fn load(
        &self,
        ctx: &Context,
        uri: &str,
        texture_options: TextureOptions,
        size_hint: SizeHint,
    ) -> TextureLoadResult {
        let svg_size_hint = if is_svg(uri) {
            // For SVGs it's important that we render at the desired size,
            // or we might get a blurry image when we scale it up.
            // So we make the size hint a part of the cache key.
            // This might lead to a lot of extra entries for the same SVG file,
            // which is potentially wasteful of RAM, but better that than blurry images.
            Some(size_hint)
        } else {
            // For other images we just use one cache value, no matter what the size we render at.
            None
        };

        let mut state = self.state.lock();
        let State { pass_index, cache } = &mut *state;

        if let Some(texture) = cache
            .get_mut(uri)
            .and_then(|by_options| {
                by_options
                    .iter_mut()
                    .find(|(options, _)| *options == texture_options)
            })
            .and_then(|(_, bucket)| bucket.get_mut(&svg_size_hint))
        {
            texture.last_used = *pass_index;
            let texture = SizedTexture::new(texture.handle.id(), texture.source_size);
            Ok(TexturePoll::Ready { texture })
        } else {
            match ctx.try_load_image(uri, size_hint)? {
                ImagePoll::Pending { size } => Ok(TexturePoll::Pending { size }),
                ImagePoll::Ready { image } => {
                    let source_size = image.source_size;
                    let handle = ctx.load_texture(uri, image, texture_options);
                    let texture = SizedTexture::new(handle.id(), source_size);
                    let by_options = cache.entry(uri.to_owned()).or_default();
                    let bucket = if let Some(index) = by_options
                        .iter()
                        .position(|(options, _)| *options == texture_options)
                    {
                        &mut by_options[index].1
                    } else {
                        by_options.push((texture_options, Bucket::default()));
                        &mut by_options.last_mut().expect("just pushed").1
                    };
                    bucket.insert(
                        svg_size_hint,
                        Entry {
                            last_used: *pass_index,
                            source_size,
                            handle,
                        },
                    );
                    let reduce_texture_memory = ctx.options(|o| o.reduce_texture_memory);
                    if reduce_texture_memory {
                        let loaders = ctx.loaders();
                        loaders.include.forget(uri);
                        for loader in loaders.bytes.lock().iter().rev() {
                            loader.forget(uri);
                        }
                        for loader in loaders.image.lock().iter().rev() {
                            loader.forget(uri);
                        }
                    }
                    Ok(TexturePoll::Ready { texture })
                }
            }
        }
    }

    fn forget(&self, uri: &str) {
        log::trace!("forget {uri:?}");

        self.state.lock().cache.remove(uri);
    }

    fn forget_all(&self) {
        log::trace!("forget all");

        self.state.lock().cache.clear();
    }

    fn end_pass(&self, pass_index: u64) {
        let mut state = self.state.lock();
        state.pass_index = pass_index;

        let State { pass_index, cache } = &mut *state;

        cache.retain(|_uri, by_options| {
            by_options.retain_mut(|(_, bucket)| {
                if 2 <= bucket.len() {
                    // There are multiple textures of the same URI (e.g. SVGs of different scales).
                    // This could be because someone has an SVG in a resizable container,
                    // and so we get a lot of different sizes of it.
                    // This could wast VRAM, so we remove the ones that are not used in this frame.
                    bucket.retain(|_, texture| *pass_index <= texture.last_used + 1);
                }
                !bucket.is_empty()
            });
            !by_options.is_empty()
        });
    }

    fn byte_size(&self) -> usize {
        self.state
            .lock()
            .cache
            .values()
            .flatten()
            .map(|(_, bucket)| {
                bucket
                    .values()
                    .map(|texture| texture.handle.byte_size())
                    .sum::<usize>()
            })
            .sum()
    }
}

fn is_svg(uri: &str) -> bool {
    super::has_extension(uri, "svg")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load::{ImageLoadResult, ImageLoader};
    use core::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Answers every URI with a 2 by 2 image and counts the requests.
    #[derive(Default)]
    struct Counting(AtomicUsize);

    impl ImageLoader for Counting {
        fn id(&self) -> &'static str {
            "counting"
        }

        fn load(&self, _ctx: &Context, _uri: &str, _size_hint: SizeHint) -> ImageLoadResult {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(ImagePoll::Ready {
                image: Arc::new(crate::ColorImage::filled([2, 2], crate::Color32::RED)),
            })
        }

        fn forget(&self, _uri: &str) {}

        fn forget_all(&self) {}

        fn byte_size(&self) -> usize {
            0
        }
    }

    #[test]
    fn a_texture_is_made_once_per_uri_and_options() {
        let ctx = Context::default();
        let images = Arc::new(Counting::default());
        ctx.add_image_loader(Arc::clone(&images) as _);
        let loader = DefaultTextureLoader::default();
        let load = |uri: &str, options| {
            let Ok(TexturePoll::Ready { texture }) =
                loader.load(&ctx, uri, options, SizeHint::default())
            else {
                panic!("{uri} not ready");
            };
            texture.id
        };
        let linear = TextureOptions::LINEAR;
        let nearest = TextureOptions::NEAREST;
        let a = load("a.png", linear);
        assert_eq!(load("a.png", linear), a);
        assert_eq!(images.0.load(Ordering::Relaxed), 1);
        let a_nearest = load("a.png", nearest);
        assert_ne!(a_nearest, a);
        assert_eq!(load("a.png", nearest), a_nearest);
        assert_eq!(load("a.png", linear), a);
        assert_eq!(images.0.load(Ordering::Relaxed), 2);
        load("b.png", linear);
        assert_eq!(images.0.load(Ordering::Relaxed), 3);
        assert!(loader.byte_size() > 0);
        // Forgetting a URI forgets it with every option.
        loader.forget("a.png");
        load("a.png", nearest);
        load("a.png", linear);
        assert_eq!(images.0.load(Ordering::Relaxed), 5);
        load("b.png", linear);
        assert_eq!(images.0.load(Ordering::Relaxed), 5);
    }
}
