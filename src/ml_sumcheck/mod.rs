//! Sumcheck Protocol for multilinear extension

use crate::ml_sumcheck::data_structures::{ListOfProductsOfPolynomials, PolynomialInfo};
use crate::ml_sumcheck::protocol::prover::{ProverMsg, ProverState};
use crate::ml_sumcheck::protocol::verifier::SubClaim;
use crate::ml_sumcheck::protocol::IPForMLSumcheck;
use crate::rng::{Blake2s512Rng, FeedableRNG};
use ark_ff::Field;
use ark_std::marker::PhantomData;
use ark_std::vec::Vec;

pub mod protocol;

pub mod data_structures;
#[cfg(test)]
mod test;

/// Sumcheck for products of multilinear polynomial
pub struct MLSumcheck<F: Field>(#[doc(hidden)] PhantomData<F>);

/// proof generated by prover
pub type Proof<F> = Vec<ProverMsg<F>>;

impl<F: Field> MLSumcheck<F> {
    /// extract sum from the proof
    pub fn extract_sum(proof: &Proof<F>) -> F {
        proof[0].evaluations[0] + proof[0].evaluations[1]
    }

    /// generate proof of the sum of polynomial over {0,1}^`num_vars`
    ///
    /// The polynomial is represented by a list of products of polynomials along with its coefficient that is meant to be added together.
    ///
    /// This data structure of the polynomial is a list of list of `(coefficient, DenseMultilinearExtension)`.
    /// * Number of products n = `polynomial.products.len()`,
    /// * Number of multiplicands of ith product m_i = `polynomial.products[i].1.len()`,
    /// * Coefficient of ith product c_i = `polynomial.products[i].0`
    ///
    /// The resulting polynomial is
    ///
    /// $$\sum_{i=0}^{n}C_i\cdot\prod_{j=0}^{m_i}P_{ij}$$
    pub fn prove(polynomial: &ListOfProductsOfPolynomials<F>) -> Result<Proof<F>, crate::Error> {
        let mut fs_rng = Blake2s512Rng::setup();
        Self::prove_as_subprotocol(&mut fs_rng, polynomial).map(|r| r.0)
    }

    /// This function does the same thing as `prove`, but it uses a `FeedableRNG` as the transcript/to generate the
    /// verifier challenges. Additionally, it returns the prover's state in addition to the proof.
    /// Both of these allow this sumcheck to be better used as a part of a larger protocol.
    pub fn prove_as_subprotocol(
        fs_rng: &mut impl FeedableRNG<Error = crate::Error>,
        polynomial: &ListOfProductsOfPolynomials<F>,
    ) -> Result<(Proof<F>, ProverState<F>), crate::Error> {
        fs_rng.feed(&polynomial.info())?;

        let mut prover_state = IPForMLSumcheck::prover_init(polynomial);
        let mut verifier_msg = None;
        let mut prover_msgs = Vec::with_capacity(polynomial.num_variables);
        for _ in 0..polynomial.num_variables {
            let prover_msg = IPForMLSumcheck::prove_round(&mut prover_state, &verifier_msg);
            fs_rng.feed(&prover_msg)?;
            prover_msgs.push(prover_msg);
            verifier_msg = Some(IPForMLSumcheck::sample_round(fs_rng));
        }
        prover_state
            .randomness
            .push(verifier_msg.unwrap().randomness);
        Ok((prover_msgs, prover_state))
    }

    /// This function extends `prove_as_subprotocol` for use with two different lists of polynomials of different degrees
    /// Let polynomial_0 be the higher dimension polynomial.
    pub fn multi_degree_prove_as_subprotocol(
        fs_rng: &mut impl FeedableRNG<Error = crate::Error>,
        polynomial_0: &ListOfProductsOfPolynomials<F>,
        polynomial_1: &ListOfProductsOfPolynomials<F>,
    ) -> Result<((Proof<F>, Proof<F>), (ProverState<F>, ProverState<F>)), crate::Error> {
        assert!(polynomial_0.num_variables > polynomial_1.num_variables);
        fs_rng.feed(&polynomial_0.info())?;
        fs_rng.feed(&polynomial_1.info())?;

        let mut prover_0_state = IPForMLSumcheck::prover_init(polynomial_0);
        let mut prover_1_state = IPForMLSumcheck::prover_init(polynomial_1);
        let mut verifier_msg = None;
        let mut prover_0_msgs = Vec::with_capacity(polynomial_0.num_variables);
        let mut prover_1_msgs = Vec::with_capacity(polynomial_1.num_variables);
        for _ in 0..polynomial_1.num_variables {
            let prover_0_msg = IPForMLSumcheck::prove_round(&mut prover_0_state, &verifier_msg);
            let prover_1_msg = IPForMLSumcheck::prove_round(&mut prover_1_state, &verifier_msg);
            fs_rng.feed(&prover_0_msg)?;
            fs_rng.feed(&prover_1_msg)?;
            prover_0_msgs.push(prover_0_msg);
            prover_1_msgs.push(prover_1_msg);
            verifier_msg = Some(IPForMLSumcheck::sample_round(fs_rng));
        }
        prover_1_state
            .randomness
            .push(verifier_msg.clone().unwrap().randomness);
        for _ in polynomial_1.num_variables..polynomial_0.num_variables {
            let prover_msg = IPForMLSumcheck::prove_round(&mut prover_0_state, &verifier_msg);
            fs_rng.feed(&prover_msg)?;
            prover_0_msgs.push(prover_msg);
            verifier_msg = Some(IPForMLSumcheck::sample_round(fs_rng));
        }
        prover_0_state
            .randomness
            .push(verifier_msg.unwrap().randomness);
        Ok((
            (prover_0_msgs, prover_1_msgs),
            (prover_0_state, prover_1_state),
        ))
    }

    /// verify the claimed sum using the proof
    pub fn verify(
        polynomial_info: &PolynomialInfo,
        claimed_sum: F,
        proof: &Proof<F>,
    ) -> Result<SubClaim<F>, crate::Error> {
        let mut fs_rng = Blake2s512Rng::setup();
        Self::verify_as_subprotocol(&mut fs_rng, polynomial_info, claimed_sum, proof)
    }

    /// This function does the same thing as `prove`, but it uses a `FeedableRNG` as the transcript/to generate the
    /// verifier challenges. This allows this sumcheck to be used as a part of a larger protocol.
    pub fn multi_degree_verify_as_subprotocol(
        fs_rng: &mut impl FeedableRNG<Error = crate::Error>,
        polynomial_info: (&PolynomialInfo, &PolynomialInfo),
        claimed_sum: F,
        proofs: (&Proof<F>, &Proof<F>),
    ) -> Result<SubClaim<F>, crate::Error> {
        fs_rng.feed(polynomial_info.0)?;
        fs_rng.feed(polynomial_info.1)?;
        let mut verifiers_state = (
            IPForMLSumcheck::verifier_init(polynomial_info.0),
            IPForMLSumcheck::verifier_init(polynomial_info.1),
        );
        for i in 0..polynomial_info.1.num_variables {
            let provers_msg = (
                proofs.0.get(i).expect("proof is incomplete"),
                proofs.1.get(i).expect("proof is incomplete"),
            );
            fs_rng.feed(provers_msg.0)?;
            fs_rng.feed(provers_msg.1)?;
            let _verifier_msgs = IPForMLSumcheck::multi_degree_verify_round(
                ((*provers_msg.0).clone(), (*provers_msg.1).clone()),
                &mut verifiers_state,
                fs_rng,
            );
        }
        for i in polynomial_info.1.num_variables..polynomial_info.0.num_variables {
            let prover_msg = proofs.0.get(i).expect("proof is incomplete");
            fs_rng.feed(prover_msg)?;
            let _verifier_msg_0 = IPForMLSumcheck::verify_round(
                (*prover_msg).clone(),
                &mut verifiers_state.0,
                fs_rng,
            );
        }
        IPForMLSumcheck::multi_degree_check_and_generate_subclaim(verifiers_state, claimed_sum)
    }

    /// This function does the same thing as `prove`, but it uses a `FeedableRNG` as the transcript/to generate the
    /// verifier challenges. This allows this sumcheck to be used as a part of a larger protocol.
    pub fn verify_as_subprotocol(
        fs_rng: &mut impl FeedableRNG<Error = crate::Error>,
        polynomial_info: &PolynomialInfo,
        claimed_sum: F,
        proof: &Proof<F>,
    ) -> Result<SubClaim<F>, crate::Error> {
        fs_rng.feed(polynomial_info)?;
        let mut verifier_state = IPForMLSumcheck::verifier_init(polynomial_info);
        for i in 0..polynomial_info.num_variables {
            let prover_msg = proof.get(i).expect("proof is incomplete");
            fs_rng.feed(prover_msg)?;
            let _verifier_msg =
                IPForMLSumcheck::verify_round((*prover_msg).clone(), &mut verifier_state, fs_rng);
        }

        IPForMLSumcheck::check_and_generate_subclaim(verifier_state, claimed_sum)
    }
}
