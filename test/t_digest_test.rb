# frozen_string_literal: true

require 'test_helper'

module ProbabilisticMachines
  class TDigestTest < Minitest::Test
    def uniform(points, compression = 100)
      digest = TDigest.new(compression)
      (1..points).each { |value| assert_equal :added, digest.add(value) }
      digest
    end

    def test_quantiles_land_inside_one_cluster_of_the_truth
      digest = uniform(10_000)

      assert_equal 100, digest.compression
      assert_equal 10_000.0, digest.count
      assert_in_delta 5_000, digest.quantile(0.5), 10_000 / 100.0
      assert_in_delta 9_900, digest.quantile(0.99), 10_000 / 100.0
      assert_in_delta 0.5, digest.cdf(5_000.0), 0.01
      assert_equal 1.0, digest.quantile(0.0)
      assert_equal 10_000.0, digest.quantile(1.0)
    end

    def test_a_compression_below_the_minimum_raises_range_error
      assert_equal 10, TDigest::MIN_COMPRESSION
      [0, 1, 9, 65_536].each do |compression|
        assert_raises(RangeError) { TDigest.new(compression) }
      end
      assert_raises(TypeError) { TDigest.new(100.0) }
      assert_equal 10, TDigest.new(10).compression
    end

    def test_an_empty_digest_has_no_quantile
      digest = TDigest.new(100)

      assert_predicate digest, :empty?
      assert_predicate digest.quantile(0.5), :nan?
      assert_equal 0, digest.size
    end

    def test_a_weighted_value_stands_for_that_many_observations
      digest = TDigest.new(200)

      assert_equal :added, digest.add_weighted(1.0, 900.0)
      assert_equal :added, digest.add_weighted(100, 100)
      assert_equal 1_000.0, digest.count
      assert_equal [Centroid.new(1.0, 900.0), Centroid.new(100.0, 100.0)], digest.centroids
    end

    def test_an_add_outside_the_precondition_is_refused_and_changes_nothing
      digest = uniform(100)
      before = digest.encoded

      assert_equal :not_finite, digest.add(Float::NAN)
      assert_equal :not_finite, digest.add(-Float::INFINITY)
      assert_equal :not_finite, digest.add_weighted(1.0, Float::INFINITY)
      assert_equal :invalid_weight, digest.add_weighted(1.0, 0)
      assert_equal :invalid_weight, digest.add_weighted(1.0, -1.0)
      assert_equal before, digest.encoded
      assert_raises(TypeError) { digest.add('1.0') }
    end

    def test_a_total_weight_that_cannot_stay_finite_is_refused
      digest = TDigest.new(100)
      digest.add_weighted(1.0, 1e308)
      before = digest.encoded

      assert_equal :overflow, digest.add_weighted(2.0, 1e308)
      assert_equal :overflow, digest.merge(digest)
      assert_equal before, digest.encoded
    end

    def test_centroids_are_frozen_and_carry_the_weight
      digest = uniform(1_000, 50)
      centroids = digest.centroids

      assert_equal digest.size, centroids.size
      assert_predicate centroids.first, :frozen?
      assert_instance_of Centroid, centroids.first
      assert_in_delta digest.count, centroids.sum(&:weight), 0.000_1
    end

    def test_compress_keeps_the_total
      digest = uniform(500, 10)
      count = digest.count

      assert_nil digest.compress
      assert_equal count, digest.count
      assert_operator digest.size, :<=, 100
    end

    def test_encoded_decode_round_trips
      digest = uniform(10_000)
      blob = digest.encoded

      assert_equal Encoding::BINARY, blob.encoding
      assert_equal 31 + (digest.size * 16), blob.bytesize
      assert_equal blob, TDigest.decode(blob).encoded
    end

    def test_decode_answers_refusal_symbols
      assert_equal :truncated, TDigest.decode('')
      assert_equal :wrong_structure, TDigest.decode(HyperLogLog.new(1024).encoded)
      assert_equal :malformed, TDigest.decode("\xd1\x64\x00".b + ("\x00".b * 28))

      blob = uniform(3, 10).encoded

      assert_equal :truncated, TDigest.decode(blob.byteslice(0, blob.bytesize - 16))
      [0, 1, 9].each do |compression|
        tampered = blob.dup
        tampered[1, 2] = [compression].pack('S<')

        assert_equal :malformed, TDigest.decode(tampered)
      end
    end

    # Values, weights, quantiles and cdf points are exactly a Float or an
    # Integer, read natively: a Rational (once taken as a Float) or an
    # object answering to_f is TypeError, and no to_f runs. NaN and the
    # infinities still reach the core, which refuses them as :not_finite.
    def test_reals_are_floats_or_integers_read_natively
      digest = TDigest.new(100)
      to_f = Object.new.tap { |object| def object.to_f = 1.0 }
      [1r, to_f, Complex(1, 0), nil].each do |bad|
        assert_equal 'value must be a Float or Integer', assert_raises(TypeError) { digest.add(bad) }.message
        assert_equal 'value must be a Float or Integer', assert_raises(TypeError) { digest.add_weighted(bad, 1) }.message
        assert_equal 'weight must be a Float or Integer', assert_raises(TypeError) { digest.add_weighted(1, bad) }.message
        assert_equal 'q must be a Float or Integer', assert_raises(TypeError) { digest.quantile(bad) }.message
        assert_equal 'value must be a Float or Integer', assert_raises(TypeError) { digest.cdf(bad) }.message
      end
      assert digest.empty?
      assert_equal :not_finite, digest.add(Float::NAN)
      assert_equal :not_finite, digest.add(2**1024)
      Integer.alias_method(:__real_to_f, :to_f)
      Integer.define_method(:to_f) { raise 'Integer#to_f called' }
      assert_equal :added, digest.add(3)
      assert_equal :added, digest.add_weighted(5, 1)
      assert_equal [3.0, 0.0], [digest.quantile(0), digest.cdf(1)]
    ensure
      Integer.alias_method(:to_f, :__real_to_f)
      Integer.remove_method(:__real_to_f)
    end

    def test_merge_folds_both_streams
      left = TDigest.new(100)
      right = TDigest.new(100)
      (1..5_000).each { |value| left.add(value) }
      (5_001..10_000).each { |value| right.add(value) }

      assert_equal :merged, left.merge(right)
      assert_equal 10_000.0, left.count
      assert_in_delta 5_000, left.quantile(0.5), 10_000 / 100.0
    end

    def test_merge_of_another_compression_is_refused_and_changes_nothing
      digest = uniform(100)
      before = digest.encoded

      assert_equal :wrong_shape, digest.merge(uniform(100, 200))
      assert_equal before, digest.encoded
    end

    def test_merge_with_the_receiver_doubles_the_count
      digest = uniform(1_000)

      assert_equal :merged, digest.merge(digest)
      assert_equal 2_000.0, digest.count
      assert_in_delta 500, digest.quantile(0.5), 1_000 / 100.0
    end
  end
end
