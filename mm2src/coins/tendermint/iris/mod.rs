pub(crate) const CREATE_HTLC_TYPE_URL: &str = "/irismod.htlc.MsgCreateHTLC";
pub(crate) const CLAIM_HTLC_TYPE_URL: &str = "/irismod.htlc.MsgClaimHTLC";

// pub(crate) const MINT_TOKEN_TYPE_URL: &str = "/irismod/token/MsgMintToken";
// pub(crate) const BURN_TOKEN_TYPE_URL: &str = "/irismod/token/MsgBurnToken";

pub(crate) mod htlc;
pub(crate) mod htlc_proto;
