use crate::{
    signature::{identity::Identity, BlsSignature},
    CeremonyError, Engine, Powers, Tau, G2,
};
use serde::{Deserialize, Serialize};
use tracing::instrument;

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contribution {
    #[serde(flatten)]
    pub powers:        Powers,
    pub pot_pubkey:    G2,
    pub bls_signature: BlsSignature,
}

impl Contribution {
    /// Check if the contribution has any entropy added.
    #[must_use]
    pub fn has_entropy(&self) -> bool {
        self.pot_pubkey != G2::one()
    }

    /// Adds entropy to this contribution. Can be called multiple times.
    /// The entropy is consumed and the blob is zeroized after use.
    #[instrument(level = "info", skip_all, , fields(n1=self.powers.g1.len(), n2=self.powers.g2.len()))]
    pub fn add_tau<E: Engine>(
        &mut self,
        tau: &Tau,
        identity: &Identity,
    ) -> Result<(), CeremonyError> {
        // Validate points after computation to contribute faster

        // Add powers of tau
        E::add_tau_g1(tau, &mut self.powers.g1)?;
        E::add_tau_g2(tau, &mut self.powers.g2)?;
        let mut temp = [G2::one(), self.pot_pubkey];
        E::add_tau_g2(tau, &mut temp)?;
        self.bls_signature = BlsSignature::sign::<E>(identity.to_string().as_bytes(), tau);
        self.pot_pubkey = temp[1];

        Ok(())
    }

    /// Performs validations in the contribution.
    #[instrument(level = "info", skip_all, , fields(n1=self.powers.g1.len(), n2=self.powers.g2.len()))]
    pub fn validate<E: Engine>(&mut self) -> Result<(), CeremonyError> {
        // Validate points
        E::validate_g1(&self.powers.g1)?;
        E::validate_g2(&self.powers.g2)?;
        E::validate_g2(&[self.pot_pubkey])?;
        Ok(())
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::{
        group::tests::{invalid_g1, invalid_g2},
        DefaultEngine, G1,
    };

    pub fn valid_contribution() -> Contribution {
        Contribution {
            powers:        Powers {
                g1: vec![G1::one()],
                g2: vec![G2::one()],
            },
            pot_pubkey:    G2::one(),
            bls_signature: BlsSignature::empty(),
        }
    }

    pub fn invalid_g1_contribution() -> Contribution {
        Contribution {
            powers:        Powers {
                g1: vec![invalid_g1()],
                g2: vec![G2::one()],
            },
            pot_pubkey:    G2::one(),
            bls_signature: BlsSignature::empty(),
        }
    }

    pub fn invalid_g2_contribution() -> Contribution {
        Contribution {
            powers:        Powers {
                g1: vec![G1::one()],
                g2: vec![invalid_g2()],
            },
            pot_pubkey:    G2::one(),
            bls_signature: BlsSignature::empty(),
        }
    }

    pub fn invalid_pot_pubkey_contribution() -> Contribution {
        Contribution {
            powers:        Powers {
                g1: vec![G1::one()],
                g2: vec![G2::one()],
            },
            pot_pubkey:    invalid_g2(),
            bls_signature: BlsSignature::empty(),
        }
    }

    #[test]
    fn test_validate() {
        assert!(matches!(
            invalid_g1_contribution().validate::<DefaultEngine>(),
            Err(CeremonyError::InvalidG1Power(_, _))
        ));
        assert!(matches!(
            invalid_g2_contribution().validate::<DefaultEngine>(),
            Err(CeremonyError::InvalidG2Power(_, _))
        ));
        assert!(matches!(
            invalid_pot_pubkey_contribution().validate::<DefaultEngine>(),
            Err(CeremonyError::InvalidG2Power(_, _))
        ));
        assert!(valid_contribution().validate::<DefaultEngine>().is_ok());
    }

    #[test]
    fn contribution_json() {
        let value = Contribution {
            powers:        Powers::new(2, 4),
            pot_pubkey:    G2::one(),
            bls_signature: BlsSignature::empty(),
        };
        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "numG1Powers": 2,
                "numG2Powers": 4,
                "powersOfTau": {
                    "G1Powers": [
                    "0x97f1d3a73197d7942695638c4fa9ac0fc3688c4f9774b905a14e3a3f171bac586c55e83ff97a1aeffb3af00adb22c6bb",
                    "0x97f1d3a73197d7942695638c4fa9ac0fc3688c4f9774b905a14e3a3f171bac586c55e83ff97a1aeffb3af00adb22c6bb"
                    ],
                    "G2Powers": [
                    "0x93e02b6052719f607dacd3a088274f65596bd0d09920b61ab5da61bbdc7f5049334cf11213945d57e5ac7d055d042b7e024aa2b2f08f0a91260805272dc51051c6e47ad4fa403b02b4510b647ae3d1770bac0326a805bbefd48056c8c121bdb8",
                    "0x93e02b6052719f607dacd3a088274f65596bd0d09920b61ab5da61bbdc7f5049334cf11213945d57e5ac7d055d042b7e024aa2b2f08f0a91260805272dc51051c6e47ad4fa403b02b4510b647ae3d1770bac0326a805bbefd48056c8c121bdb8",
                    "0x93e02b6052719f607dacd3a088274f65596bd0d09920b61ab5da61bbdc7f5049334cf11213945d57e5ac7d055d042b7e024aa2b2f08f0a91260805272dc51051c6e47ad4fa403b02b4510b647ae3d1770bac0326a805bbefd48056c8c121bdb8",
                    "0x93e02b6052719f607dacd3a088274f65596bd0d09920b61ab5da61bbdc7f5049334cf11213945d57e5ac7d055d042b7e024aa2b2f08f0a91260805272dc51051c6e47ad4fa403b02b4510b647ae3d1770bac0326a805bbefd48056c8c121bdb8"
                    ]
                },
                "potPubkey": "0x93e02b6052719f607dacd3a088274f65596bd0d09920b61ab5da61bbdc7f5049334cf11213945d57e5ac7d055d042b7e024aa2b2f08f0a91260805272dc51051c6e47ad4fa403b02b4510b647ae3d1770bac0326a805bbefd48056c8c121bdb8",
                "blsSignature": ""
            })
        );
        let deser = serde_json::from_value::<Contribution>(json).unwrap();
        assert_eq!(deser, value);
    }
}
