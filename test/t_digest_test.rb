# frozen_string_literal: true

require 'test_helper'

module ProbabilisticMachines
  class TDigestTest < Minitest::Test
    def test_quantiles_land_inside_one_cluster_of_the_truth
      digest = TDigest.new(compression: 100)
      (1..10_000).each { |value| digest.add(value.to_f) }

      assert_equal 100, digest.compression
      assert_equal 10_000.0, digest.count
      assert_in_delta 5_000, digest.quantile(0.5), 10_000 / 100.0
      assert_in_delta 9_900, digest.quantile(0.99), 10_000 / 100.0
      assert_in_delta 0.5, digest.cdf(5_000.0), 0.01
      assert_equal 1.0, digest.quantile(0.0)
      assert_equal 10_000.0, digest.quantile(1.0)
    end

    def test_an_empty_digest_has_no_quantile
      digest = TDigest.new(compression: 100)

      assert_predicate digest, :empty?
      assert_predicate digest.quantile(0.5), :nan?
      assert_equal 0, digest.size
    end

    def test_a_weighted_value_stands_for_that_many_observations
      digest = TDigest.new(compression: 200)
      digest.add(1.0, weight: 900.0)
      digest.add(100.0, weight: 100.0)

      assert_equal 1_000.0, digest.count
      assert_equal 2, digest.size
      assert_equal [900.0, 100.0], digest.centroids.map(&:weight)
      assert_equal 1.0, digest.quantile(0.0)
      assert_equal 100.0, digest.quantile(1.0)
    end

    def test_centroids_are_frozen_and_carry_the_weight
      digest = TDigest.new(compression: 50)
      (1..1_000).each { |value| digest.add(value.to_f) }
      centroids = digest.centroids

      assert_equal digest.size, centroids.size
      assert_predicate centroids.first, :frozen?
      assert_instance_of Centroid, centroids.first
      assert_in_delta digest.count, centroids.sum(&:weight), 0.000_1
    end

    def test_dump_load_round_trips
      digest = TDigest.new(compression: 100)
      (1..10_000).each { |value| digest.add(value.to_f) }
      blob = digest.dump

      assert_equal Encoding::BINARY, blob.encoding
      assert_equal 31 + (digest.size * 16), blob.bytesize

      restored = TDigest.load(blob)

      assert_equal [100, digest.size, digest.count], [restored.compression, restored.size, restored.count]
      assert_in_delta digest.quantile(0.99), restored.quantile(0.99), 0.000_1
    end

    def test_load_rejects_a_blob_it_did_not_write
      assert_raises(ArgumentError) { TDigest.load('') }
      assert_raises(ArgumentError) { TDigest.load(HyperLogLog.new(registers: 1024).dump) }
      assert_raises(ArgumentError) { TDigest.load("\xd1\x64\x00".b + ("\x00".b * 30)) }
    end

    def test_load_rejects_a_blob_that_lost_whole_centroids
      digest = TDigest.new(compression: 10)
      [1.0, 2.0, 3.0].each { |value| digest.add(value) }
      blob = digest.dump

      assert_raises(ArgumentError) { TDigest.load(blob.byteslice(0, 31)) }
      assert_raises(ArgumentError) { TDigest.load(blob.byteslice(0, blob.bytesize - 16)) }
      assert_equal 3.0, TDigest.load(blob).count
    end

    def test_merge_folds_both_streams
      left = TDigest.new(compression: 100)
      right = TDigest.new(compression: 100)
      (1..5_000).each { |value| left.add(value.to_f) }
      (5_001..10_000).each { |value| right.add(value.to_f) }

      assert_same left, left.merge(right)
      assert_equal 10_000.0, left.count
      assert_in_delta 5_000, left.quantile(0.5), 10_000 / 100.0
    end

    def test_merge_with_the_receiver_doubles_the_count
      digest = TDigest.new(compression: 100)
      (1..1_000).each { |value| digest.add(value.to_f) }

      digest.merge(digest)

      assert_equal 2_000.0, digest.count
      assert_in_delta 500, digest.quantile(0.5), 1_000 / 100.0
    end

    def test_a_compression_that_holds_no_quantile_is_rejected
      assert_raises(ArgumentError) { TDigest.new(compression: 0) }
    end

    def test_a_blob_claiming_a_compression_the_binding_refuses_does_not_load
      digest = TDigest.new(compression: 10)
      digest.add(1.0)

      [0, 1, 9].each do |refused|
        blob = digest.dump
        blob[1, 2] = [refused].pack('S<')

        assert_raises(ArgumentError) { TDigest.load(blob) }
      end
      assert_equal 10, TDigest.load(digest.dump).compression
    end

    def test_a_total_weight_that_cannot_stay_finite_is_rejected
      digest = TDigest.new(compression: 100)
      digest.add(1.0, weight: 1e308)

      assert_raises(ArgumentError) { digest.add(2.0, weight: 1e308) }
      assert_predicate digest.count, :finite?
    end
  end
end
