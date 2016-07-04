// Copyright 2015, 2016 Ethcore (UK) Ltd.
// This file is part of Parity.

// Parity is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Parity is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Parity.  If not, see <http://www.gnu.org/licenses/>.

use std::collections::HashMap;
use sha3::SHA3_EMPTY;
use rlp::untrusted_rlp::BasicDecoder;
use rlp::{UntrustedRlp, View, PayloadInfo, DecoderError, Decoder, Compressible, SHA3_NULL_RLP, encode, ElasticArray1024, Stream, RlpStream};

/// Stores RLPs used for compression
struct InvalidRlpSwapper {
	invalid_to_valid: HashMap<Vec<u8>, Vec<u8>>,
	valid_to_invalid: HashMap<Vec<u8>, Vec<u8>>,
}

impl InvalidRlpSwapper {
	/// Construct a swapper from a list of common RLPs
	fn new(rlps_to_swap: Vec<Vec<u8>>) -> Self {
		if rlps_to_swap.len() > 0x80 {
			panic!("Invalid usage, only 128 RLPs can be swappable.");
		}
		let mut invalid_to_valid = HashMap::new();
		let mut valid_to_invalid = HashMap::new();
		for (i, rlp) in rlps_to_swap.iter().enumerate() {
			let invalid = vec!(0x81, i as u8); 	
			invalid_to_valid.insert(invalid.clone(), rlp.clone());
			valid_to_invalid.insert(rlp.to_owned(), invalid);
		}
		InvalidRlpSwapper {
			invalid_to_valid: invalid_to_valid,
			valid_to_invalid: valid_to_invalid
		}
	}
	/// Get a valid RLP corresponding to an invalid one
	fn get_valid(&self, invalid_rlp: &[u8]) -> Option<&[u8]> {
		self.invalid_to_valid.get(invalid_rlp).map(|v| v.as_slice())
	}
	/// Get an invalid RLP corresponding to a valid one
	fn get_invalid(&self, valid_rlp: &[u8]) -> Option<&[u8]> {
		self.valid_to_invalid.get(valid_rlp).map(|v| v.as_slice())
	}
}

lazy_static! {
	static ref INVALID_RLP_SWAPPER: InvalidRlpSwapper = InvalidRlpSwapper::new(vec![encode(&SHA3_NULL_RLP).to_vec(), encode(&SHA3_EMPTY).to_vec()]);
}

impl<'a> Compressible for UntrustedRlp<'a> {
	fn swap<F>(&self, swapper: F) -> ElasticArray1024<u8>
	where F: Fn(&[u8]) -> Option<&[u8]> {
		if self.is_data() {
			let raw = self.as_raw();
			let mut result = ElasticArray1024::new();
			result.append_slice(swapper(raw).unwrap_or(raw));
			return result;
		};
		let raw_rlp = self.iter()
			.map(|subrlp| {
					let b = subrlp.as_raw();
					swapper(b).unwrap_or(b)
      	})
    	.fold(ElasticArray1024::new(), |mut acc, slice| { acc.append_slice(slice); acc });
  	let mut rlp = RlpStream::new_list(self.item_count());
  	rlp.append_raw(&raw_rlp, self.item_count());
  	rlp.drain()
	}

	fn compress(&self) -> ElasticArray1024<u8> {
		self.swap(|b| INVALID_RLP_SWAPPER.get_invalid(b))
	}
	fn decompress(&self) -> ElasticArray1024<u8> {
		self.swap(|b| INVALID_RLP_SWAPPER.get_valid(b))
	}
}

struct DecompressingDecoder<'a> {
	rlp: UntrustedRlp<'a>,
	swapper: &'a INVALID_RLP_SWAPPER,
}

impl<'a> DecompressingDecoder<'a> {
	pub fn new(rlp: UntrustedRlp<'a>) -> DecompressingDecoder<'a> {
		DecompressingDecoder {
			rlp: rlp,
			swapper: &INVALID_RLP_SWAPPER,
		}
	}

	/// Return first item info.
	fn payload_info(bytes: &[u8]) -> Result<PayloadInfo, DecoderError> {
		let item = try!(PayloadInfo::from(bytes));
		match item.header_len.checked_add(item.value_len) { 
			Some(x) if x <= bytes.len() => Ok(item), 
			_ => Err(DecoderError::RlpIsTooShort), 
		}
	}
}

impl<'a> Decoder for DecompressingDecoder<'a> {
	fn read_value<T, F>(&self, f: &F) -> Result<T, DecoderError>
	where F: Fn(&[u8]) -> Result<T, DecoderError> {
		match BasicDecoder::new(self.rlp.clone()).read_value(f) {
			// Try again with decompression.
			Err(DecoderError::RlpInvalidIndirection) => {
				let decompressed = self.rlp.decompress();
				BasicDecoder::new(UntrustedRlp::new(&decompressed)).read_value(f)
			},
			// Just return on valid RLP.
			x => x,
		}
	}

	fn as_raw(&self) -> &[u8] {
		self.rlp.as_raw()
	}

	fn as_rlp(&self) -> &UntrustedRlp {
		&self.rlp
	}
}

#[test]
fn invalid_rlp_swapper() {
	let swapper = InvalidRlpSwapper::new(vec![vec![0x83, b'c', b'a', b't'], vec![0x83, b'd', b'o', b'g']]);
	let invalid_rlp = vec![vec![0x81, 0x00], vec![0x81, 0x01]];
	assert_eq!(Some(invalid_rlp[0].as_slice()), swapper.get_invalid(&[0x83, b'c', b'a', b't']));
	assert_eq!(None, swapper.get_invalid(&[0x83, b'b', b'a', b't']));
	assert_eq!(Some(vec![0x83, b'd', b'o', b'g'].as_slice()), swapper.get_valid(&invalid_rlp[1]));
}
