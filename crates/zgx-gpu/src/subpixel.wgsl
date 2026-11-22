// ClearType glyphs (`Shading::Subpixel`): appended to draw.wgsl in a module
// with dual-source blending enabled (`QuadPipelines::subpixel`). Each
// subpixel blends apart: the target takes the text's colour by its own
// channel's coverage (`dwrite3`), `color * coverage + target * (1 - coverage)`.
// A flat quad among them (an underline) blends by its alpha alone.
struct SubpixelOut { @location(0) @blend_src(0) color:vec4<f32>, @location(0) @blend_src(1) coverage:vec4<f32> }
@fragment fn fs_subpixel(v:Basic)->SubpixelOut {
 if v.texture<0.5 {return SubpixelOut(vec4(v.color.rgb,1.),vec4(v.color.a));}
 let tex=textureSample(atlas,tex_sampler,v.uv);
 let c=dwrite3(tex.rgb,v.color.rgb)*v.color.a;
 return SubpixelOut(vec4(v.color.rgb,1.),vec4(c,(c.r+c.g+c.b)/3.));
}
