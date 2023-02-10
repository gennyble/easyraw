use crate::{
	colorspace::{BayerRgb, Colorspace, LinRgb},
	RollingRandom,
};

use super::Image;

impl<T: Copy + Clone> Image<T, BayerRgb> {
	/// Crops the raw image down, removing parts we're supposed to.
	///
	/// A camera may cover part of a sensor to gather black level information
	/// or noise information, and this function removes those parts so we can
	/// get just the image itself
	pub fn crop(&mut self) {
		let crop = if let Some(crop) = self.metadata.crop.as_ref() {
			*crop
		} else {
			return;
		};

		let new_width = self.width - (crop.left + crop.right);
		let new_height = self.height - (crop.top + crop.bottom);
		let new_size = new_width * new_height;

		//TODO: gen- we do not need to allocate again here. We, in theory, can
		// do this all in the already existing vec
		let mut image = Vec::with_capacity(new_size);
		for row in 0..new_height {
			let row_x = row + crop.top;

			let start = row_x * self.width + crop.left;
			let end = start + new_width;
			image.extend_from_slice(&self.data[start..end]);
		}

		self.width = new_width;
		self.height = new_height;
		self.data = image;
		self.metadata.crop = None;
		self.metadata.cfa = self.metadata.cfa.shift(crop.left, crop.top);
	}

	fn color_at_i(&self, i: usize) -> CfaColor {
		CfaColor::from(self.metadata.cfa.color_at(i % self.width, i / self.width))
	}

	pub fn debayer(self) -> Image<T, LinRgb> {
		let mut rgb = vec![self.data[0]; self.width * self.height * 3];

		let cfa = self.metadata.cfa.clone();
		let mut rr = RollingRandom::new();

		#[rustfmt::skip]
		let options = [
			(-1, -1), (0, -1), (1, -1),
			(-1, 0),  /*skip*/ (1, 0),
			(-1, 1),  (0, 1),  (1, 1)
		];

		let get = |p: (usize, usize)| -> T { self.data[self.width * p.1 + p.0] };
		let mut set = |x: usize, y: usize, clr: CfaColor, v: T| {
			rgb[(self.width * y + x) * LinRgb::COMPONENTS + clr.rgb_index()] = v;
		};

		//TODO: gen- care about the edges of the image
		// We're staying away from the borders for now so we can handle them special later
		for x in 1..self.width - 1 {
			for y in 1..self.height - 1 {
				let options = options.clone().into_iter().map(|(x_off, y_off)| {
					let x = (x as isize + x_off) as usize;
					let y = (y as isize + y_off) as usize;
					(CfaColor::from(cfa.color_at(x, y)), x, y)
				});

				match CfaColor::from(cfa.color_at(x, y)) {
					#[rustfmt::skip]
					CfaColor::Red => {
						set(x, y, CfaColor::Red, get((x, y)));
						set(x, y, CfaColor::Green, get(pick_color(&mut rr, options.clone(), CfaColor::Green)));
						set(x, y, CfaColor::Blue, get(pick_color(&mut rr, options.clone(), CfaColor::Blue)));
					}
					#[rustfmt::skip]
					CfaColor::Blue => {
						set(x, y, CfaColor::Red, get(pick_color(&mut rr, options.clone(), CfaColor::Red)));
						set(x, y, CfaColor::Blue, get((x, y)));
						set(x, y, CfaColor::Green, get(pick_color(&mut rr, options.clone(), CfaColor::Green)));
					}
					#[rustfmt::skip]
					CfaColor::Green => {
						set(x, y, CfaColor::Red, get(pick_color(&mut rr, options.clone(), CfaColor::Red)));
						set(x, y, CfaColor::Blue, get(pick_color(&mut rr, options.clone(), CfaColor::Blue)));
						set(x, y, CfaColor::Green, get((x, y)));
					}
					CfaColor::Emerald => unreachable!(),
				}
			}
		}

		Image {
			width: self.width,
			height: self.height,
			metadata: self.metadata,
			data: rgb,
			phantom: Default::default(),
		}
	}
}

impl Image<f32, BayerRgb> {
	pub fn whitebalance(&mut self) {
		let wb = self.metadata.whitebalance;
		for (i, light) in self.data.iter_mut().enumerate() {
			match CfaColor::from(self.metadata.cfa.color_at(i % self.width, i / self.width)) {
				CfaColor::Red => *light = *light as f32 * wb[0],
				CfaColor::Green => *light = *light as f32 * wb[1],
				CfaColor::Blue => *light = *light as f32 * wb[2],
				CfaColor::Emerald => unreachable!(),
			}
		}
	}
}

impl Image<u16, BayerRgb> {
	pub fn whitebalance(&mut self) {
		let wb = self.metadata.whitebalance;
		for (i, light) in self.data.iter_mut().enumerate() {
			/*match CfaColor::from(self.metadata.cfa.color_at(i % self.width, i / self.width)) {
				CfaColor::Red => *light = (*light as f32 * wb[0]) as u16,
				CfaColor::Green => *light = (*light as f32 * wb[1]) as u16,
				CfaColor::Blue => *light = (*light as f32 * wb[2]) as u16,
				CfaColor::Emerald => unreachable!(),
			}*/
			*light = (*light as f32
				* wb[self.metadata.cfa.color_at(i % self.width, i / self.width)]) as u16;
		}
	}
}

impl Image<u8, BayerRgb> {
	pub fn whitebalance(&mut self) {
		let wb = self.metadata.whitebalance;
		for (i, light) in self.data.iter_mut().enumerate() {
			match CfaColor::from(self.metadata.cfa.color_at(i % self.width, i / self.width)) {
				CfaColor::Red => *light = (*light as f32 * wb[0]) as u8,
				CfaColor::Green => *light = (*light as f32 * wb[1]) as u8,
				CfaColor::Blue => *light = (*light as f32 * wb[2]) as u8,
				CfaColor::Emerald => unreachable!(),
			}
		}
	}
}

#[inline]
fn pick_color<I>(roll: &mut RollingRandom, options: I, color: CfaColor) -> (usize, usize)
where
	I: Iterator<Item = (CfaColor, usize, usize)>,
{
	let colors: Vec<(CfaColor, usize, usize)> =
		options.filter(|(clr, _, _)| *clr == color).collect();
	let random = roll.random_u8() % colors.len() as u8;
	let red = &colors[random as usize];

	(red.1, red.2)
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum CfaColor {
	Red,
	Green,
	Blue,
	Emerald,
}

impl CfaColor {
	pub fn rgb_index(&self) -> usize {
		match self {
			CfaColor::Red => 0,
			CfaColor::Green => 1,
			CfaColor::Blue => 2,
			CfaColor::Emerald => unreachable!(),
		}
	}
}

impl From<usize> for CfaColor {
	fn from(value: usize) -> Self {
		match value {
			0 => CfaColor::Red,
			1 => CfaColor::Green,
			2 => CfaColor::Blue,
			3 => CfaColor::Emerald,
			_ => unreachable!(),
		}
	}
}
